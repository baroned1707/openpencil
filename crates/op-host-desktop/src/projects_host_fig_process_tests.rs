//! The child-process plumbing of the real converter, driven with a tiny shell
//! script standing in for `openpencil-desktop`: it is started with
//! `--convert-fig <in> <out>` and acts on the first line of the input file.

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

static SEQ: AtomicU32 = AtomicU32::new(0);

const SCRIPT: &str = r#"#!/bin/sh
[ "$1" = "--convert-fig" ] || exit 9
in="$2"; out="$3"
case "$(head -n 1 "$in")" in
  ok) printf converted > "$out"; exit 0 ;;
  exists) exit 3 ;;
  fail) echo "noise" >&2; echo "convert-fig: bad kiwi header" >&2; exit 1 ;;
  silent) exit 0 ;;
  noisy) i=0; while [ $i -lt 5000 ]; do echo "warning line $i of noise" >&2; i=$((i+1)); done; exit 1 ;;
  hang) : > "$(dirname "$out")/.$(basename "$out").$$.1.tmp"; exec sleep 60 ;;
esac
exit 8
"#;

struct Setup {
    dir: PathBuf,
    converter: ProcessConverter,
}

fn setup() -> Setup {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-fig-process-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("fake-converter.sh");
    std::fs::write(&script, SCRIPT).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Setup {
        converter: ProcessConverter::new(script),
        dir,
    }
}

fn run(setup: &Setup, behaviour: &str) -> (Box<dyn ConversionRun>, PathBuf) {
    let input = setup.dir.join(format!("{behaviour}.fig"));
    std::fs::write(&input, format!("{behaviour}\n")).unwrap();
    let output = setup.dir.join(format!("{behaviour}.op"));
    (setup.converter.start(&input, &output).unwrap(), output)
}

fn wait(run: &mut Box<dyn ConversionRun>) -> ConvertOutcome {
    let started = Instant::now();
    loop {
        if let Some(outcome) = run.poll() {
            return outcome;
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "child never ended"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_child_that_writes_its_output_and_exits_zero_is_done() {
    let setup = setup();
    let (mut child, output) = run(&setup, "ok");
    assert_eq!(wait(&mut child), ConvertOutcome::Done);
    assert_eq!(std::fs::read(&output).unwrap(), b"converted");
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn exit_three_means_the_output_name_was_taken() {
    let setup = setup();
    let (mut child, _) = run(&setup, "exists");
    assert_eq!(wait(&mut child), ConvertOutcome::OutputExists);
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn a_failing_child_reports_the_last_line_of_its_stderr() {
    let setup = setup();
    let (mut child, _) = run(&setup, "fail");
    assert_eq!(
        wait(&mut child),
        ConvertOutcome::Failed("bad kiwi header".to_string())
    );
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn exit_zero_without_an_output_file_is_a_failure() {
    let setup = setup();
    let (mut child, output) = run(&setup, "silent");
    assert_eq!(
        wait(&mut child),
        ConvertOutcome::Failed("no output was written".to_string())
    );
    assert!(!output.exists());
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn a_child_with_a_flood_of_stderr_is_not_blocked_by_a_full_pipe() {
    let setup = setup();
    let (mut child, _) = run(&setup, "noisy");
    assert!(matches!(wait(&mut child), ConvertOutcome::Failed(_)));
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn a_killed_child_is_gone_and_leaves_no_temp_file() {
    let setup = setup();
    let (mut child, output) = run(&setup, "hang");
    // Give the script a moment to create its temp file.
    let temp_exists = |dir: &Path| {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
    };
    let started = Instant::now();
    while !temp_exists(&setup.dir) {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(child.poll(), None, "still running");
    child.kill();
    assert!(
        !temp_exists(&setup.dir),
        "the killed child's temp file is removed"
    );
    assert!(!output.exists());
    assert_eq!(child.poll(), None);
    let _ = std::fs::remove_dir_all(setup.dir);
}

#[test]
fn the_queue_kills_a_real_child_on_timeout_and_goes_on() {
    use super::super::fig::{FigJobs, FigLimits, JobStatus};
    let setup = setup();
    let root = setup.dir.canonicalize().unwrap();
    for (name, body) in [("a-hang", "hang\n"), ("b-ok", "ok\n")] {
        std::fs::write(root.join(format!("{name}.fig")), body).unwrap();
    }
    let converter = setup.converter;
    let mut queue = FigJobs::with_limits(
        Arc::new(converter),
        FigLimits {
            timeout: Duration::from_millis(300),
            ..FigLimits::default()
        },
    );
    let hang = root.join("a-hang.fig");
    queue.enqueue(hang.clone(), root.clone(), false);
    queue.enqueue(root.join("b-ok.fig"), root.clone(), false);
    // The clock handed to the queue is synthetic, so a slow machine cannot
    // time out the healthy child: first let it finish, then move past the limit.
    let begin = Instant::now();
    let ok = root.join("b-ok.fig");
    while queue.status().get(&ok) != Some(&JobStatus::Done) {
        queue.pump(begin);
        assert!(
            begin.elapsed() < Duration::from_secs(20),
            "healthy child never ended"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    queue.pump(begin + Duration::from_millis(301));
    assert!(!queue.has_work());
    assert_eq!(
        queue.status().get(&hang),
        Some(&JobStatus::Failed("timed out".to_string()))
    );
    assert_eq!(
        std::fs::read(root.join("b-ok.op")).ok(),
        Some(b"converted".to_vec()),
        "{:?}",
        queue.status()
    );
    assert!(!root.join("a-hang.op").exists());
    let _ = std::fs::remove_dir_all(root);
}
