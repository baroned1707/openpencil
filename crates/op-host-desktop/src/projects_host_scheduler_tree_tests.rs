//! Opened projects on the scan scheduler: walked on the worker thread, only when
//! asked, and told apart from "some older scan finished" by a serial number.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread::ThreadId;

const DEADLINE: Duration = Duration::from_secs(10);

fn wait_until(mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < DEADLINE, "timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Counts runs, remembers its threads and the roots it was given, and can be
/// held open.
#[derive(Default)]
struct Probe {
    hold: AtomicBool,
    calls: AtomicUsize,
    threads: Mutex<Vec<ThreadId>>,
    roots: Mutex<Vec<PathBuf>>,
}

impl Probe {
    fn job(self: &Arc<Self>, found: &'static str) -> ScanJob {
        let probe = Arc::clone(self);
        Arc::new(move |roots| {
            probe.calls.fetch_add(1, Ordering::SeqCst);
            probe
                .threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            probe.roots.lock().unwrap().extend(roots.iter().cloned());
            let started = Instant::now();
            while probe.hold.load(Ordering::SeqCst) && started.elapsed() < DEADLINE {
                std::thread::sleep(Duration::from_millis(1));
            }
            roots
                .iter()
                .map(|root| {
                    (
                        root.clone(),
                        DesignListing {
                            files: vec![root.join(found)],
                            status: ScanStatus::Ok,
                        },
                    )
                })
                .collect()
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn designs_root() -> PathBuf {
    PathBuf::from("/p/inferred")
}

fn opened_root() -> PathBuf {
    PathBuf::from("/p/opened")
}

fn both() -> BTreeSet<PathBuf> {
    [designs_root(), opened_root()].into()
}

fn scheduler(designs: &Arc<Probe>, tree: &Arc<Probe>, every: Duration) -> ScanScheduler {
    ScanScheduler::with_timing(designs.job("designs/a.op"), every, SLOW_AFTER)
        .with_tree_job(tree.job("deep/er/b.op"))
}

fn drain(scheduler: &mut ScanScheduler) {
    wait_until(|| {
        scheduler.poll(&both(), || false);
        !scheduler.in_flight()
    });
}

#[test]
fn scan_runs_off_the_ui_thread() {
    let (designs, tree) = (Arc::new(Probe::default()), Arc::new(Probe::default()));
    let mut scheduler = scheduler(&designs, &tree, Duration::from_secs(3600));
    scheduler.request_tree_scan(&opened_root());
    tree.hold.store(true, Ordering::SeqCst);

    let frame = Instant::now();
    for _ in 0..20 {
        scheduler.poll(&both(), || false);
    }
    assert!(
        frame.elapsed() < Duration::from_secs(1),
        "frames must not wait for the walk"
    );
    wait_until(|| tree.calls() == 1);
    assert_ne!(
        tree.threads.lock().unwrap()[0],
        std::thread::current().id(),
        "the walk runs on the worker, not the UI thread"
    );
    tree.hold.store(false, Ordering::SeqCst);
    drain(&mut scheduler);

    assert_eq!(
        *tree.roots.lock().unwrap(),
        [opened_root()],
        "the walk got the opened root only"
    );
    assert_eq!(
        *designs.roots.lock().unwrap(),
        [designs_root()],
        "the designs job got the inferred root only"
    );
    assert_eq!(
        scheduler.files().get(&opened_root()),
        Some(&vec![opened_root().join("deep/er/b.op")])
    );
}

#[test]
fn the_periodic_tick_rescans_designs_folders_but_never_re_walks_an_opened_project() {
    let (designs, tree) = (Arc::new(Probe::default()), Arc::new(Probe::default()));
    let mut scheduler = scheduler(&designs, &tree, Duration::ZERO);
    scheduler.request_tree_scan(&opened_root());
    drain(&mut scheduler);
    assert_eq!((designs.calls(), tree.calls()), (1, 1));
    for _ in 0..5 {
        scheduler.poll(&both(), || true);
        drain(&mut scheduler);
    }
    assert!(designs.calls() > 1, "the tick still rescans designs/");
    assert_eq!(
        tree.calls(),
        1,
        "an opened project is walked on request only"
    );

    scheduler.request_tree_scan(&opened_root());
    drain(&mut scheduler);
    assert_eq!(tree.calls(), 2, "a refresh walks it again");
}

#[test]
fn a_serial_says_when_the_requested_walk_has_finished() {
    let (designs, tree) = (Arc::new(Probe::default()), Arc::new(Probe::default()));
    let mut scheduler = scheduler(&designs, &tree, Duration::from_secs(3600));
    tree.hold.store(true, Ordering::SeqCst);
    let first = scheduler.request_tree_scan(&opened_root());
    scheduler.poll(&both(), || false);
    assert!(scheduler.in_flight());
    assert!(scheduler.completed_serial() < first);

    // A refresh asked for while a walk runs needs the NEXT scan, not this one.
    let second = scheduler.request_tree_scan(&opened_root());
    assert!(second > first);
    tree.hold.store(false, Ordering::SeqCst);
    drain(&mut scheduler);
    assert!(scheduler.completed_serial() >= first);
    wait_until(|| {
        scheduler.poll(&both(), || false);
        scheduler.completed_serial() >= second
    });
    assert_eq!(tree.calls(), 2);
}

#[test]
fn forgetting_a_root_drops_its_listing_and_stops_scanning_it() {
    let (designs, tree) = (Arc::new(Probe::default()), Arc::new(Probe::default()));
    let mut scheduler = scheduler(&designs, &tree, Duration::ZERO);
    scheduler.request_tree_scan(&opened_root());
    drain(&mut scheduler);
    assert!(scheduler.files().contains_key(&opened_root()));
    let version = scheduler.version();
    scheduler.forget_root(&opened_root());
    assert!(!scheduler.files().contains_key(&opened_root()));
    assert_eq!(scheduler.status(&opened_root()), None);
    assert!(scheduler.version() > version, "the dock must rebuild");

    let only_designs: BTreeSet<PathBuf> = [designs_root()].into();
    scheduler.poll(&only_designs, || true);
    wait_until(|| {
        scheduler.poll(&only_designs, || false);
        !scheduler.in_flight()
    });
    assert_eq!(tree.calls(), 1, "never walked again");
}

#[test]
fn a_walk_is_abandoned_later_than_a_designs_listing() {
    let (designs, tree) = (Arc::new(Probe::default()), Arc::new(Probe::default()));
    // Everything held open; designs listings are abandoned after 5 ms, walks
    // after a minute.
    designs.hold.store(true, Ordering::SeqCst);
    tree.hold.store(true, Ordering::SeqCst);
    let mut scheduler = ScanScheduler::with_timing(
        designs.job("designs/a.op"),
        Duration::from_secs(3600),
        Duration::from_millis(5),
    )
    .with_tree_job(tree.job("b.op"))
    .with_tree_slow_after(Duration::from_secs(60));
    scheduler.poll(&[designs_root()].into(), || false);
    wait_until(|| {
        scheduler.poll(&[designs_root()].into(), || false);
        scheduler.status(&designs_root()) == Some(ScanStatus::Slow)
    });
    designs.hold.store(false, Ordering::SeqCst);
    drain_one(&mut scheduler, &[designs_root()].into());

    scheduler.request_tree_scan(&opened_root());
    scheduler.poll(&both(), || false);
    std::thread::sleep(Duration::from_millis(50));
    scheduler.poll(&both(), || false);
    assert_ne!(
        scheduler.status(&opened_root()),
        Some(ScanStatus::Slow),
        "a walk gets its own, longer allowance"
    );
    tree.hold.store(false, Ordering::SeqCst);
    drain(&mut scheduler);
}

fn drain_one(scheduler: &mut ScanScheduler, roots: &BTreeSet<PathBuf>) {
    wait_until(|| {
        scheduler.poll(roots, || false);
        !scheduler.in_flight()
    });
}
