//! The scan scheduler: the frame never waits for the disk, at most one scan is
//! alive, and a scan that stalls leaves the last listing in place.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread::ThreadId;

const TEST_DEADLINE: Duration = Duration::from_secs(10);

/// A scan job that can be held open, counts its calls and remembers which
/// threads ran it. Each call returns the next prepared listing (the last one
/// repeats).
struct ScriptedJob {
    hold: AtomicBool,
    calls: AtomicUsize,
    running: AtomicUsize,
    max_running: AtomicUsize,
    threads: Mutex<Vec<ThreadId>>,
    listings: Mutex<Vec<Vec<PathBuf>>>,
}

impl ScriptedJob {
    fn new(listings: Vec<Vec<PathBuf>>) -> Arc<Self> {
        Arc::new(Self {
            hold: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            running: AtomicUsize::new(0),
            max_running: AtomicUsize::new(0),
            threads: Mutex::new(Vec::new()),
            listings: Mutex::new(listings),
        })
    }

    fn job(self: &Arc<Self>) -> ScanJob {
        let script = Arc::clone(self);
        Arc::new(move |roots| {
            let call = script.calls.fetch_add(1, Ordering::SeqCst);
            let now_running = script.running.fetch_add(1, Ordering::SeqCst) + 1;
            script.max_running.fetch_max(now_running, Ordering::SeqCst);
            script
                .threads
                .lock()
                .unwrap()
                .push(std::thread::current().id());
            let started = Instant::now();
            while script.hold.load(Ordering::SeqCst) && started.elapsed() < TEST_DEADLINE {
                std::thread::sleep(Duration::from_millis(1));
            }
            let files = {
                let listings = script.listings.lock().unwrap();
                listings
                    .get(call)
                    .or_else(|| listings.last())
                    .cloned()
                    .unwrap_or_default()
            };
            script.running.fetch_sub(1, Ordering::SeqCst);
            roots
                .iter()
                .map(|root| {
                    (
                        root.clone(),
                        DesignListing {
                            files: files.clone(),
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

fn roots() -> BTreeSet<PathBuf> {
    [PathBuf::from("/p/shop")].into()
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < TEST_DEADLINE, "timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn poll(scheduler: &mut ScanScheduler) -> bool {
    scheduler.poll(&roots(), || true)
}

#[test]
fn scan_runs_off_the_ui_thread_and_a_slow_scan_does_not_block_a_frame() {
    let script = ScriptedJob::new(vec![vec![PathBuf::from("/p/shop/designs/a.op")]]);
    script.hold.store(true, Ordering::SeqCst);
    let mut scheduler = ScanScheduler::new(script.job());

    let frame = Instant::now();
    for _ in 0..20 {
        poll(&mut scheduler);
    }
    assert!(
        frame.elapsed() < Duration::from_secs(1),
        "twenty frames took {:?} while the scan was held open",
        frame.elapsed()
    );
    wait_until(|| script.calls() == 1);
    assert_ne!(
        script.threads.lock().unwrap()[0],
        std::thread::current().id(),
        "the scan must not run on the calling (UI) thread"
    );
    assert!(scheduler.in_flight());

    script.hold.store(false, Ordering::SeqCst);
    wait_until(|| poll(&mut scheduler));
    assert_eq!(
        scheduler.files().get(Path::new("/p/shop")),
        Some(&vec![PathBuf::from("/p/shop/designs/a.op")])
    );
}

#[test]
fn only_one_scan_is_in_flight() {
    let script = ScriptedJob::new(vec![Vec::new()]);
    script.hold.store(true, Ordering::SeqCst);
    let mut scheduler = ScanScheduler::new(script.job());
    for _ in 0..50 {
        scheduler.request_now();
        poll(&mut scheduler);
    }
    wait_until(|| script.calls() >= 1);
    assert_eq!(
        script.calls(),
        1,
        "requests during a scan must not start another"
    );
    script.hold.store(false, Ordering::SeqCst);
    // The queued request runs once the first scan returned.
    wait_until(|| {
        poll(&mut scheduler);
        script.calls() >= 2
    });
    wait_until(|| {
        poll(&mut scheduler);
        scheduler.is_idle()
    });
    assert_eq!(script.max_running.load(Ordering::SeqCst), 1);
    assert_eq!(script.calls(), 2);
}

#[test]
fn abandoned_slow_scan_keeps_the_last_listing() {
    let first = vec![PathBuf::from("/p/shop/designs/a.op")];
    let second = vec![
        PathBuf::from("/p/shop/designs/a.op"),
        PathBuf::from("/p/shop/designs/b.op"),
    ];
    let script = ScriptedJob::new(vec![first.clone(), second.clone()]);
    let mut scheduler = ScanScheduler::with_timing(
        script.job(),
        Duration::from_secs(3600),
        Duration::from_millis(40),
    );
    wait_until(|| poll(&mut scheduler));
    assert_eq!(scheduler.status(Path::new("/p/shop")), Some(ScanStatus::Ok));

    script.hold.store(true, Ordering::SeqCst);
    scheduler.request_now();
    poll(&mut scheduler);
    wait_until(|| {
        poll(&mut scheduler) && scheduler.status(Path::new("/p/shop")) == Some(ScanStatus::Slow)
    });
    assert_eq!(
        scheduler.files().get(Path::new("/p/shop")),
        Some(&first),
        "a slow scan keeps the last known listing"
    );
    let calls_when_abandoned = script.calls();
    for _ in 0..20 {
        scheduler.request_now();
        poll(&mut scheduler);
    }
    assert_eq!(
        script.calls(),
        calls_when_abandoned,
        "no new scan starts while the stuck one has not returned"
    );

    script.hold.store(false, Ordering::SeqCst);
    wait_until(|| {
        poll(&mut scheduler);
        scheduler.status(Path::new("/p/shop")) == Some(ScanStatus::Ok)
            && scheduler.files().get(Path::new("/p/shop")) == Some(&second)
    });
}

#[test]
fn a_periodic_rescan_needs_a_focused_window() {
    let script = ScriptedJob::new(vec![Vec::new()]);
    let mut scheduler = ScanScheduler::with_timing(script.job(), Duration::ZERO, SLOW_AFTER);
    // The first scan is a request, so it needs no focus; drain it unfocused so
    // the same frame cannot also start a periodic one.
    wait_until(|| {
        scheduler.poll(&roots(), || false);
        scheduler.is_idle() && script.calls() == 1
    });
    // Due by the clock, but the window is not focused.
    for _ in 0..10 {
        scheduler.poll(&roots(), || false);
    }
    assert_eq!(script.calls(), 1);
    // A request ignores focus.
    scheduler.request_now();
    scheduler.poll(&roots(), || false);
    wait_until(|| script.calls() == 2);
    wait_until(|| poll(&mut scheduler) || scheduler.is_idle());
    // Focused again and due: another scan.
    wait_until(|| {
        poll(&mut scheduler);
        script.calls() >= 3
    });
}

#[test]
fn focus_is_only_asked_when_a_periodic_rescan_is_due() {
    let script = ScriptedJob::new(vec![Vec::new()]);
    let mut scheduler =
        ScanScheduler::with_timing(script.job(), Duration::from_secs(3600), SLOW_AFTER);
    wait_until(|| poll(&mut scheduler));
    let asked = AtomicUsize::new(0);
    for _ in 0..10 {
        scheduler.poll(&roots(), || {
            asked.fetch_add(1, Ordering::SeqCst);
            true
        });
    }
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

#[test]
fn a_scan_that_panics_is_reported_unreadable() {
    let job: ScanJob = Arc::new(|_| panic!("scan blew up"));
    let mut scheduler = ScanScheduler::new(job);
    wait_until(|| poll(&mut scheduler) && !scheduler.in_flight());
    wait_until(|| {
        poll(&mut scheduler);
        scheduler.status(Path::new("/p/shop")) == Some(ScanStatus::Unreadable(ErrorKind::Other))
    });
}
