//! The conversion queue with a fake converter: how many run at once, what a
//! timeout does to the batch, what is refused at start and what is never
//! overwritten. The fake is deliberately dumb: it writes whatever path it is
//! handed with a plain `std::fs::write`, so a wrong path chosen by the queue
//! shows up as damaged files, not as a polite refusal.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

static SEQ: AtomicU32 = AtomicU32::new(0);

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-fig-jobs-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn fig(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"fig").unwrap();
    path
}

#[derive(Clone)]
enum Behaviour {
    /// Writes the output and succeeds once released.
    Succeed,
    /// Fails with this reason once released.
    Fail(&'static str),
    /// Never finishes on its own.
    Hang,
    /// Creates this file (somebody else took the name), then reports it.
    TakeTheNameFirst(PathBuf),
    /// Writes this many bytes, then succeeds.
    WriteBytes(usize),
}

#[derive(Default)]
struct Shared {
    started: Vec<(PathBuf, PathBuf)>,
    running: usize,
    peak: usize,
    killed: usize,
}

struct Fake {
    shared: Arc<Mutex<Shared>>,
    release: Arc<AtomicBool>,
    behaviour: Box<dyn Fn(&Path) -> Behaviour + Send + Sync>,
}

impl Fake {
    fn new(behaviour: impl Fn(&Path) -> Behaviour + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::default(),
            release: Arc::new(AtomicBool::new(true)),
            behaviour: Box::new(behaviour),
        })
    }

    /// Runs that finish only after `release()`.
    fn held(behaviour: impl Fn(&Path) -> Behaviour + Send + Sync + 'static) -> Arc<Self> {
        let fake = Self::new(behaviour);
        fake.release.store(false, Ordering::SeqCst);
        fake
    }

    fn release(&self) {
        self.release.store(true, Ordering::SeqCst);
    }

    fn started(&self) -> Vec<(PathBuf, PathBuf)> {
        self.shared.lock().unwrap().started.clone()
    }

    fn peak(&self) -> usize {
        self.shared.lock().unwrap().peak
    }

    fn killed(&self) -> usize {
        self.shared.lock().unwrap().killed
    }
}

impl FigConverter for Fake {
    fn start(&self, input: &Path, output: &Path) -> io::Result<Box<dyn ConversionRun>> {
        let mut shared = self.shared.lock().unwrap();
        shared
            .started
            .push((input.to_path_buf(), output.to_path_buf()));
        shared.running += 1;
        shared.peak = shared.peak.max(shared.running);
        Ok(Box::new(FakeRun {
            shared: Arc::clone(&self.shared),
            release: Arc::clone(&self.release),
            behaviour: (self.behaviour)(input),
            output: output.to_path_buf(),
            ended: false,
        }))
    }
}

struct FakeRun {
    shared: Arc<Mutex<Shared>>,
    release: Arc<AtomicBool>,
    behaviour: Behaviour,
    output: PathBuf,
    ended: bool,
}

impl FakeRun {
    fn end(&mut self) {
        if !self.ended {
            self.ended = true;
            self.shared.lock().unwrap().running -= 1;
        }
    }
}

impl ConversionRun for FakeRun {
    fn poll(&mut self) -> Option<ConvertOutcome> {
        if matches!(self.behaviour, Behaviour::Hang) || !self.release.load(Ordering::SeqCst) {
            return None;
        }
        self.end();
        Some(match &self.behaviour {
            Behaviour::Succeed => {
                std::fs::write(&self.output, b"converted").unwrap();
                ConvertOutcome::Done
            }
            Behaviour::Fail(reason) => ConvertOutcome::Failed((*reason).to_string()),
            Behaviour::TakeTheNameFirst(taken) => {
                std::fs::write(taken, b"someone else's file").unwrap();
                ConvertOutcome::OutputExists
            }
            Behaviour::WriteBytes(count) => {
                std::fs::write(&self.output, vec![b'x'; *count]).unwrap();
                ConvertOutcome::Done
            }
            Behaviour::Hang => unreachable!(),
        })
    }

    fn kill(&mut self) {
        if !self.ended {
            self.shared.lock().unwrap().killed += 1;
        }
        self.end();
    }
}

fn jobs(fake: &Arc<Fake>) -> FigJobs {
    FigJobs::new(Arc::clone(fake) as Arc<dyn FigConverter>)
}

/// Pump until nothing runs or waits, collecting what finished.
fn drain(jobs: &mut FigJobs) -> Vec<Finished> {
    let mut finished = Vec::new();
    for _ in 0..200 {
        finished.extend(jobs.pump(Instant::now()));
        if !jobs.has_work() {
            return finished;
        }
    }
    panic!("the queue never drained");
}

#[test]
fn at_most_two_conversions_run_at_once() {
    let root = project("parallel");
    let fake = Fake::held(|_| Behaviour::Succeed);
    let mut queue = jobs(&fake);
    let figs: Vec<PathBuf> = (0..5).map(|n| fig(&root, &format!("f{n}.fig"))).collect();
    for path in &figs {
        assert!(queue.enqueue(path.clone(), root.clone(), false));
    }
    queue.pump(Instant::now());
    assert_eq!(queue.running_count(), 2, "two run, three wait");
    assert_eq!(fake.started().len(), 2);
    // Releasing lets the next ones start as slots free up; never more than two.
    fake.release();
    let finished = drain(&mut queue);
    assert_eq!(finished.len(), 5);
    assert_eq!(fake.started().len(), 5);
    assert_eq!(fake.peak(), 2, "never more than two at the same time");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn timeout_marks_failed_and_batch_continues() {
    let root = project("timeout");
    let stuck = fig(&root, "a-stuck.fig");
    let fine = fig(&root, "b-fine.fig");
    let also_fine = fig(&root, "c-fine.fig");
    let fake = Fake::new(|input| {
        if input.ends_with("a-stuck.fig") {
            Behaviour::Hang
        } else {
            Behaviour::Succeed
        }
    });
    let mut queue = FigJobs::with_limits(
        Arc::clone(&fake) as Arc<dyn FigConverter>,
        FigLimits {
            timeout: Duration::from_millis(500),
            ..FigLimits::default()
        },
    );
    for path in [&stuck, &fine, &also_fine] {
        queue.enqueue(path.clone(), root.clone(), false);
    }
    let begin = Instant::now();
    let mut finished = queue.pump(begin);
    assert_eq!(queue.running_count(), 2, "the stuck one holds a slot");
    // The healthy ones finish; the stuck one is still within its time.
    finished.extend(queue.pump(begin + Duration::from_millis(100)));
    finished.extend(queue.pump(begin + Duration::from_millis(200)));
    assert_eq!(
        finished.len(),
        2,
        "the stuck file does not block the others"
    );
    assert_eq!(queue.status().get(&stuck), Some(&JobStatus::Converting));
    // Past the timeout it is killed and fails; the batch is already done.
    queue.pump(begin + Duration::from_millis(600));
    assert_eq!(
        queue.status().get(&stuck),
        Some(&JobStatus::Failed("timed out".to_string()))
    );
    assert_eq!(fake.killed(), 1);
    assert!(!queue.has_work());
    assert_eq!(queue.status().get(&fine), Some(&JobStatus::Done));
    assert_eq!(queue.status().get(&also_fine), Some(&JobStatus::Done));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn one_failure_never_stops_the_batch() {
    let root = project("failure");
    let bad = fig(&root, "a-bad.fig");
    let good = fig(&root, "b-good.fig");
    let fake = Fake::new(|input| {
        if input.ends_with("a-bad.fig") {
            Behaviour::Fail("bad kiwi header")
        } else {
            Behaviour::Succeed
        }
    });
    let mut queue = jobs(&fake);
    queue.enqueue(bad.clone(), root.clone(), false);
    queue.enqueue(good.clone(), root.clone(), false);
    let finished = drain(&mut queue);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].output, root.join("b-good.op"));
    assert_eq!(
        queue.status().get(&bad),
        Some(&JobStatus::Failed("bad kiwi header".to_string()))
    );
    // A failed file can be tried again.
    assert!(queue.enqueue(bad, root.clone(), false));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn convert_never_overwrites_an_existing_op() {
    let root = project("overwrite");
    let with_op = fig(&root, "a.fig");
    std::fs::write(root.join("a.op"), b"precious").unwrap();
    let with_folder = fig(&root, "b.fig");
    std::fs::create_dir(root.join("b.op")).unwrap();
    let fake = Fake::new(|_| Behaviour::Succeed);
    let mut queue = jobs(&fake);
    queue.enqueue(with_op, root.clone(), false);
    queue.enqueue(with_folder, root.clone(), false);
    let finished = drain(&mut queue);

    assert_eq!(std::fs::read(root.join("a.op")).unwrap(), b"precious");
    let started = fake.started();
    assert_eq!(started.len(), 1, "a.fig already has its .op: not converted");
    assert_eq!(
        started[0].1,
        root.join("b-converted.op"),
        "a name taken by a folder gets a new name"
    );
    assert!(root.join("b.op").is_dir());
    assert_eq!(
        std::fs::read(root.join("b-converted.op")).unwrap(),
        b"converted"
    );
    let outputs: Vec<PathBuf> = finished.into_iter().map(|done| done.output).collect();
    assert_eq!(
        outputs,
        [root.join("a.op"), root.join("b-converted.op")],
        "the existing file is what a click opens"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_name_taken_in_the_race_is_never_replaced() {
    let root = project("race");
    let listed = fig(&root, "a.fig");
    std::fs::create_dir(root.join("a.op")).unwrap();
    // Between choosing `a-converted.op` and creating it, someone else did.
    let fake = Fake::new({
        let taken = root.join("a-converted.op");
        move |_| Behaviour::TakeTheNameFirst(taken.clone())
    });
    let mut queue = jobs(&fake);
    queue.enqueue(listed.clone(), root.clone(), false);
    // Every attempt fails to create its name: the queue moves on to the next
    // free one and finally gives up, never replacing what is there.
    let finished = drain(&mut queue);
    assert!(finished.is_empty());
    assert_eq!(
        std::fs::read(root.join("a-converted.op")).unwrap(),
        b"someone else's file"
    );
    let outputs: Vec<PathBuf> = fake.started().into_iter().map(|run| run.1).collect();
    assert_eq!(outputs[0], root.join("a-converted.op"));
    assert_eq!(
        outputs[1],
        root.join("a-converted-2.op"),
        "the next free name"
    );
    assert!(matches!(
        queue.status().get(&listed),
        Some(JobStatus::Failed(_))
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_escape_fig_is_refused() {
    let root = project("toctou");
    let outside = project("toctou-outside");
    let listed = fig(&root, "inside.fig");
    let fake = Fake::new(|_| Behaviour::Succeed);
    let mut queue = jobs(&fake);
    queue.enqueue(listed.clone(), root.clone(), false);
    // The scan saw a regular file; before the job starts it becomes a link
    // that leaves the project.
    std::fs::remove_file(&listed).unwrap();
    let secret = fig(&outside, "secret.fig");
    std::os::unix::fs::symlink(&secret, &listed).unwrap();
    let finished = drain(&mut queue);
    assert!(finished.is_empty());
    assert!(
        fake.started().is_empty(),
        "the converter never saw the path"
    );
    assert_eq!(
        queue.status().get(&listed),
        Some(&JobStatus::Failed("outside the project".to_string()))
    );
    assert!(!outside.join("secret.op").exists());
    assert!(!root.join("inside.op").exists());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn an_oversized_output_is_deleted_and_fails() {
    let root = project("big-output");
    let listed = fig(&root, "a.fig");
    let fake = Fake::new(|_| Behaviour::WriteBytes(100));
    let mut queue = FigJobs::with_limits(
        Arc::clone(&fake) as Arc<dyn FigConverter>,
        FigLimits {
            max_output_bytes: 10,
            ..FigLimits::default()
        },
    );
    queue.enqueue(listed.clone(), root.clone(), false);
    let finished = drain(&mut queue);
    assert!(finished.is_empty());
    assert!(!root.join("a.op").exists());
    assert_eq!(
        queue.status().get(&listed),
        Some(&JobStatus::Failed("output too large".to_string()))
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn closing_a_project_stops_its_conversions_and_forgets_its_queue() {
    let root = project("close");
    let other = project("close-other");
    let running = fig(&root, "a.fig");
    let waiting = fig(&root, "b.fig");
    let third = fig(&root, "c.fig");
    let kept = fig(&other, "k.fig");
    let fake = Fake::held(|_| Behaviour::Succeed);
    let mut queue = jobs(&fake);
    for (path, home) in [
        (&running, &root),
        (&waiting, &root),
        (&third, &root),
        (&kept, &other),
    ] {
        queue.enqueue(path.clone(), home.clone(), false);
    }
    queue.pump(Instant::now());
    assert_eq!(queue.running_count(), 2);
    queue.forget_root(&root);
    assert_eq!(fake.killed(), 2);
    assert_eq!(queue.running_count(), 0);
    assert!(queue.status().keys().all(|path| !path.starts_with(&root)));
    // The other project's file is still queued and now gets a slot.
    fake.release();
    let finished = drain(&mut queue);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].fig, kept);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn enqueueing_a_file_that_is_already_converting_does_nothing() {
    let root = project("twice");
    let listed = fig(&root, "a.fig");
    let fake = Fake::held(|_| Behaviour::Succeed);
    let mut queue = jobs(&fake);
    assert!(queue.enqueue(listed.clone(), root.clone(), true));
    assert!(
        !queue.enqueue(listed, root.clone(), false),
        "a double click"
    );
    queue.pump(Instant::now());
    fake.release();
    let finished = drain(&mut queue);
    assert_eq!(finished.len(), 1);
    assert!(
        finished[0].open_when_done,
        "the first request's choice wins"
    );
    let _ = std::fs::remove_dir_all(root);
}
