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

#[test]
fn a_rename_is_shown_at_once_and_a_scan_that_started_before_it_cannot_undo_it() {
    let old = PathBuf::from("/p/shop/designs/card.op");
    let new = PathBuf::from("/p/shop/designs/hero.op");
    let other = PathBuf::from("/p/shop/designs/other.op");
    // Both scans read the folder as it was BEFORE the rename.
    let script = ScriptedJob::new(vec![vec![old.clone(), other.clone()]]);
    script.hold.store(true, Ordering::SeqCst);
    let mut scheduler = ScanScheduler::with_timing(script.job(), RESCAN_EVERY, SLOW_AFTER);
    scheduler.poll(&roots(), || true);
    assert!(scheduler.in_flight());

    // The rename lands while that scan is still reading.
    scheduler.rename_listed(&old, &new);
    script.hold.store(false, Ordering::SeqCst);
    wait_until(|| {
        scheduler.poll(&roots(), || false);
        !scheduler.in_flight()
    });
    let listed = &scheduler.files()[&PathBuf::from("/p/shop")];
    assert!(listed.contains(&new), "{listed:?}");
    assert!(
        !listed.contains(&old),
        "the old row did not come back: {listed:?}"
    );
    assert!(listed.contains(&other));
}

#[test]
fn a_rename_updates_the_cached_listing_and_its_version() {
    let old = PathBuf::from("/p/shop/designs/card.op");
    let new = PathBuf::from("/p/shop/designs/hero.op");
    let script = ScriptedJob::new(vec![vec![old.clone()]]);
    let mut scheduler = ScanScheduler::with_timing(script.job(), RESCAN_EVERY, SLOW_AFTER);
    wait_until(|| {
        scheduler.poll(&roots(), || true);
        !scheduler.files().is_empty()
    });
    let version = scheduler.version();
    scheduler.rename_listed(&old, &new);
    assert_eq!(
        scheduler.files()[&PathBuf::from("/p/shop")],
        vec![new.clone()]
    );
    assert!(scheduler.version() > version, "the tree rebuilds");
    // Renaming a file nobody listed changes nothing.
    let version = scheduler.version();
    scheduler.rename_listed(&PathBuf::from("/elsewhere/x.op"), &new);
    assert_eq!(scheduler.version(), version);
}

/// A scan job over a shared, editable "disk": every call reads `disk` as it is
/// then, and counts itself.
struct FakeDisk {
    files: Mutex<Vec<PathBuf>>,
    calls: AtomicUsize,
}

impl FakeDisk {
    fn new(files: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            files: Mutex::new(files.iter().map(PathBuf::from).collect()),
            calls: AtomicUsize::new(0),
        })
    }

    fn job(self: &Arc<Self>) -> ScanJob {
        let disk = Arc::clone(self);
        Arc::new(move |roots| {
            disk.calls.fetch_add(1, Ordering::SeqCst);
            let files = disk.files.lock().unwrap().clone();
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

    fn set(&self, files: &[&str]) {
        *self.files.lock().unwrap() = files.iter().map(PathBuf::from).collect();
    }
}

/// Poll until the scan that is running (if any) has landed.
fn land(scheduler: &mut ScanScheduler, active: bool) {
    wait_until(|| {
        scheduler.poll(&roots(), || active);
        !scheduler.in_flight() && !scheduler.wanted
    });
}

/// Invariant: a rescan that finds exactly what is already listed changes nothing
/// the frame can see (no version bump, so no tree rebuild and no snapshot churn).
#[test]
fn unchanged_rescan_does_not_touch_the_snapshot() {
    let disk = FakeDisk::new(&["/p/shop/designs/a.op", "/p/shop/designs/b.op"]);
    let mut scheduler = ScanScheduler::with_timing(disk.job(), Duration::ZERO, SLOW_AFTER);
    land(&mut scheduler, true);
    let version = scheduler.version();
    assert!(version > 0, "the first scan is news");

    for _ in 0..4 {
        scheduler.request_rescan_all();
        land(&mut scheduler, true);
    }
    assert!(disk.calls() >= 5, "the rescans really ran");
    assert_eq!(scheduler.version(), version, "and changed nothing");

    disk.set(&["/p/shop/designs/a.op"]);
    scheduler.request_rescan_all();
    land(&mut scheduler, true);
    assert!(scheduler.version() > version, "a real change is news");
}

/// Invariant: an idle app that is not focused (or whose dock is folded) does no
/// file-system work, however long it sits; and it starts again the moment the
/// caller says a background rescan is wanted.
#[test]
fn idle_unfocused_app_does_no_file_system_work() {
    let disk = FakeDisk::new(&["/p/shop/designs/a.op"]);
    let mut scheduler = ScanScheduler::with_timing(disk.job(), Duration::ZERO, SLOW_AFTER);
    land(&mut scheduler, false);
    let after_first = disk.calls();
    assert_eq!(after_first, 1, "the first listing");

    for _ in 0..200 {
        scheduler.poll(&roots(), || false);
    }
    std::thread::sleep(Duration::from_millis(20));
    for _ in 0..200 {
        scheduler.poll(&roots(), || false);
    }
    assert_eq!(disk.calls(), after_first, "no scan while unfocused");
    assert!(!scheduler.in_flight());
    assert_eq!(scheduler.next_background_due().is_some(), true);

    wait_until(|| {
        scheduler.poll(&roots(), || true);
        disk.calls() > after_first
    });
}

/// Invariant: the background tick waits at least `every`, and a scan that took
/// long pushes the next one further out (a huge repository is not walked back
/// to back).
#[test]
fn background_rescans_are_spaced_and_back_off_after_a_slow_scan() {
    let disk = FakeDisk::new(&["/p/shop/designs/a.op"]);
    let mut scheduler =
        ScanScheduler::with_timing(disk.job(), Duration::from_secs(3600), SLOW_AFTER);
    land(&mut scheduler, true);
    let calls = disk.calls();
    for _ in 0..50 {
        scheduler.poll(&roots(), || true);
    }
    assert_eq!(disk.calls(), calls, "not due for an hour");
    let due = scheduler.next_background_due().expect("a due time");
    assert!(due > Instant::now() + Duration::from_secs(3000));

    let mut scheduler = ScanScheduler::with_timing(disk.job(), Duration::ZERO, SLOW_AFTER);
    land(&mut scheduler, true);
    scheduler.last_duration = Duration::from_secs(10);
    let due = scheduler.next_background_due().expect("a due time");
    assert!(
        due >= scheduler.last_finished.unwrap() + Duration::from_secs(30),
        "three times the last scan"
    );
}

/// Invariant: a scan that started before the app changed the disk must not undo
/// that change when it lands (the row would flicker back, or a just-created row
/// would be missing and its rename dropped).
#[test]
fn a_scan_already_running_does_not_undo_a_patch_made_meanwhile() {
    let script = ScriptedJob::new(vec![vec![
        PathBuf::from("/p/shop/designs/a.op"),
        PathBuf::from("/p/shop/designs/gone.op"),
    ]]);
    let mut scheduler = ScanScheduler::with_timing(script.job(), Duration::ZERO, SLOW_AFTER);
    land(&mut scheduler, true);
    script.hold.store(true, Ordering::SeqCst);
    scheduler.request_rescan_all();
    scheduler.poll(&roots(), || true);
    assert!(scheduler.in_flight());

    // While that scan reads the old disk, the app removes one file and creates one.
    scheduler.remove_listed(Path::new("/p/shop/designs/gone.op"));
    scheduler.insert_listed(
        Path::new("/p/shop"),
        Path::new("/p/shop/designs/Untitled.op"),
    );
    script.hold.store(false, Ordering::SeqCst);
    land(&mut scheduler, true);

    let listed = scheduler.files().get(Path::new("/p/shop")).unwrap();
    assert!(!listed.contains(&PathBuf::from("/p/shop/designs/gone.op")));
    assert!(listed.contains(&PathBuf::from("/p/shop/designs/Untitled.op")));

    // The next scan reads the real disk and settles it.
    scheduler.request_rescan_all();
    land(&mut scheduler, true);
    assert!(scheduler
        .files()
        .get(Path::new("/p/shop"))
        .unwrap()
        .contains(&PathBuf::from("/p/shop/designs/gone.op")));
}

/// Invariant: every scan stats the watched (open) paths and reports the ones
/// that are gone; one that is present, or merely unreadable, is not missing.
#[test]
fn a_scan_reports_which_open_files_are_gone() {
    let dir = std::env::temp_dir().join(format!("openpencil-watched-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let present = dir.join("here.op");
    std::fs::write(&present, "{}").unwrap();
    let gone = dir.join("gone.op");

    let disk = FakeDisk::new(&[]);
    let mut scheduler = ScanScheduler::with_timing(disk.job(), Duration::ZERO, SLOW_AFTER);
    scheduler.set_watched(vec![present.clone(), gone.clone()]);
    land(&mut scheduler, true);
    assert_eq!(scheduler.missing().iter().collect::<Vec<_>>(), [&gone]);

    // It comes back (a Save As onto the same path): the next scan clears it.
    std::fs::write(&gone, "{}").unwrap();
    scheduler.request_rescan_all();
    land(&mut scheduler, true);
    assert!(scheduler.missing().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
