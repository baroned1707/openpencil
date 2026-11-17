//! Runs the Projects dock's disk scan off the UI thread.
//!
//! The frame never reads a directory. Each frame it calls [`ScanScheduler::poll`],
//! which (i) drains a finished scan from the channel and (ii) starts a new one
//! on a fresh thread when one is due: when something that can change the
//! listing happened ([`ScanScheduler::request_now`]) or every few seconds while
//! the window has focus. At most one scan is ever alive.
//!
//! A scan that has not finished after `slow_after` is *abandoned*, not killed
//! (a `read_dir` stuck on a dead network drive cannot be interrupted): its
//! projects are marked [`ScanStatus::Slow`] and keep their last listing, and no
//! new scan starts until the stuck one returns. When it does return late, its
//! result is applied like any other, so the listing catches up and `Slow`
//! clears. A request that arrives meanwhile is remembered and runs next.
//!
//! Two kinds of root share the one worker. An inferred project's `designs/` folder
//! is cheap and re-read on the periodic tick. A folder the user opened is walked
//! recursively (`projects_host_scan_tree.rs`) ONLY when it was just opened, or
//! refreshed ([`ScanScheduler::request_tree_scan`]): the periodic tick never
//! re-walks a whole repository. Each started scan gets a serial number, so the
//! host can tell "the scan I asked for has finished" from "some older scan did".

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_host_services::project_root::ProjectRootCache;

use super::scan::{scan_design_dir, DesignListing, ScanStatus};
use super::scan_tree::{scan_project_tree, TreeLimits};

/// How often `<root>/designs` is re-read while nothing else changed, so a file
/// an agent created from outside the editor shows up.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(3);
/// A scan still running after this long is abandoned (marked slow).
pub(crate) const SLOW_AFTER: Duration = Duration::from_secs(5);
/// A scan that includes a recursive walk is abandoned only after this long:
/// the walk has a time budget of its own (10 s) and returns its partial result
/// by then, so this fires only when the walk is stuck inside a system call.
pub(crate) const TREE_SLOW_AFTER: Duration = Duration::from_secs(12);

/// What one scan does with a set of project roots. Replaceable so tests can
/// block, count and time it.
pub(crate) type ScanJob = Arc<dyn Fn(&[PathBuf]) -> Vec<(PathBuf, DesignListing)> + Send + Sync>;

/// The real `designs/` job: re-resolve project roots that went stale (off the UI
/// thread), then list every project's `designs/` folder.
pub(crate) fn disk_scan_job(roots: Arc<ProjectRootCache>) -> ScanJob {
    Arc::new(move |project_roots| {
        roots.refresh_stale();
        project_roots
            .iter()
            .map(|root| (root.clone(), scan_design_dir(root)))
            .collect()
    })
}

/// The real recursive job for the folders the user opened.
pub(crate) fn tree_scan_job() -> ScanJob {
    Arc::new(|project_roots| {
        project_roots
            .iter()
            .map(|root| {
                (
                    root.clone(),
                    scan_project_tree(root, &TreeLimits::default()),
                )
            })
            .collect()
    })
}

struct InFlight {
    serial: u64,
    started: Instant,
    /// Whether this scan walks an opened project (it may take longer).
    walks_trees: bool,
    results: Receiver<Vec<(PathBuf, DesignListing)>>,
    roots: Vec<PathBuf>,
    slow_reported: bool,
}

pub(crate) struct ScanScheduler {
    job: ScanJob,
    /// Walks the roots in `trees`; the same as `job` unless set.
    tree_job: ScanJob,
    every: Duration,
    slow_after: Duration,
    tree_slow_after: Duration,
    /// Roots the user opened: walked recursively, never on the periodic tick.
    trees: BTreeSet<PathBuf>,
    /// Opened roots waiting for their walk.
    dirty_trees: BTreeSet<PathBuf>,
    /// Serial of the latest scan that started, and of the latest that finished.
    started_serial: u64,
    completed_serial: u64,
    in_flight: Option<InFlight>,
    /// Something asked for a scan as soon as none is running.
    wanted: bool,
    last_finished: Option<Instant>,
    files: BTreeMap<PathBuf, Vec<PathBuf>>,
    status: BTreeMap<PathBuf, ScanStatus>,
    /// Bumped whenever `files` or `status` change, so the frame can tell.
    version: u64,
}

impl ScanScheduler {
    pub(crate) fn new(job: ScanJob) -> Self {
        Self::with_timing(job, RESCAN_EVERY, SLOW_AFTER)
    }

    pub(crate) fn with_timing(job: ScanJob, every: Duration, slow_after: Duration) -> Self {
        Self {
            tree_job: Arc::clone(&job),
            job,
            every,
            slow_after,
            tree_slow_after: TREE_SLOW_AFTER,
            trees: BTreeSet::new(),
            dirty_trees: BTreeSet::new(),
            started_serial: 0,
            completed_serial: 0,
            in_flight: None,
            wanted: true,
            last_finished: None,
            files: BTreeMap::new(),
            status: BTreeMap::new(),
            version: 0,
        }
    }

    /// Use `job` to walk the opened projects (see the module docs).
    pub(crate) fn with_tree_job(mut self, job: ScanJob) -> Self {
        self.tree_job = job;
        self
    }

    /// Override how long a scan with a recursive walk may run before it is
    /// abandoned.
    #[cfg(test)]
    pub(crate) fn with_tree_slow_after(mut self, after: Duration) -> Self {
        self.tree_slow_after = after;
        self
    }

    /// Mark `root` as an opened project and walk it. Also the refresh: call it
    /// again to walk it again. Returns the serial of the scan that will include
    /// it; the walk is done once [`Self::completed_serial`] reaches that.
    pub(crate) fn request_tree_scan(&mut self, root: &Path) -> u64 {
        self.trees.insert(root.to_path_buf());
        self.dirty_trees.insert(root.to_path_buf());
        self.wanted = true;
        self.started_serial + 1
    }

    /// Stop scanning `root` and forget its listing (a closed project).
    pub(crate) fn forget_root(&mut self, root: &Path) {
        self.trees.remove(root);
        self.dirty_trees.remove(root);
        let had_listing = self.files.remove(root).is_some();
        let had_status = self.status.remove(root).is_some();
        if had_listing || had_status {
            self.version += 1;
        }
    }

    /// Serial of the latest scan whose result was applied.
    pub(crate) fn completed_serial(&self) -> u64 {
        self.completed_serial
    }

    /// Ask for a scan as soon as none is running (a project appeared, a design
    /// was created or opened).
    pub(crate) fn request_now(&mut self) {
        self.wanted = true;
    }

    /// Whether a scan is running (or abandoned but not yet returned).
    pub(crate) fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// No scan running and none waiting.
    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.in_flight.is_none() && !self.wanted
    }

    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    pub(crate) fn files(&self) -> &BTreeMap<PathBuf, Vec<PathBuf>> {
        &self.files
    }

    pub(crate) fn status(&self, root: &Path) -> Option<ScanStatus> {
        self.status.get(root).copied()
    }

    /// One frame's worth of scan bookkeeping. Never blocks. `focused` is asked
    /// only when a periodic rescan is due. Returns whether the listing or a
    /// status changed.
    pub(crate) fn poll(
        &mut self,
        roots: &BTreeSet<PathBuf>,
        focused: impl FnOnce() -> bool,
    ) -> bool {
        let changed = self.drain();
        if self.in_flight.is_none() && !roots.is_empty() {
            let periodic_due = self
                .last_finished
                .is_none_or(|at| at.elapsed() >= self.every);
            if self.wanted || (periodic_due && focused()) {
                let designs: Vec<PathBuf> = roots
                    .iter()
                    .filter(|root| !self.trees.contains(*root))
                    .cloned()
                    .collect();
                let trees: Vec<PathBuf> = self
                    .dirty_trees
                    .iter()
                    .filter(|root| roots.contains(*root))
                    .cloned()
                    .collect();
                if designs.is_empty() && trees.is_empty() {
                    self.wanted = false;
                } else {
                    self.start(designs, trees);
                }
            }
        }
        changed
    }

    fn drain(&mut self) -> bool {
        let Some(flight) = self.in_flight.as_mut() else {
            return false;
        };
        match flight.results.try_recv() {
            Ok(results) => {
                self.completed_serial = flight.serial;
                self.in_flight = None;
                self.last_finished = Some(Instant::now());
                for (root, listing) in results {
                    self.status.insert(root.clone(), listing.status);
                    self.files.insert(root, listing.files);
                }
                self.version += 1;
                true
            }
            Err(TryRecvError::Empty) => {
                let limit = if flight.walks_trees {
                    self.tree_slow_after
                } else {
                    self.slow_after
                };
                if flight.slow_reported || flight.started.elapsed() < limit {
                    return false;
                }
                flight.slow_reported = true;
                for root in &flight.roots {
                    self.status.insert(root.clone(), ScanStatus::Slow);
                }
                self.version += 1;
                true
            }
            Err(TryRecvError::Disconnected) => {
                // The scan thread died without answering (a panic).
                let roots = std::mem::take(&mut flight.roots);
                self.completed_serial = flight.serial;
                self.in_flight = None;
                self.last_finished = Some(Instant::now());
                self.mark_all(&roots, ScanStatus::Unreadable(ErrorKind::Other));
                true
            }
        }
    }

    fn start(&mut self, designs: Vec<PathBuf>, trees: Vec<PathBuf>) {
        self.wanted = false;
        self.dirty_trees.clear();
        self.started_serial += 1;
        let serial = self.started_serial;
        let roots: Vec<PathBuf> = designs.iter().chain(&trees).cloned().collect();
        let (sender, results) = mpsc::channel();
        let job = Arc::clone(&self.job);
        let tree_job = Arc::clone(&self.tree_job);
        let walks_trees = !trees.is_empty();
        let spawned = std::thread::Builder::new()
            .name("projects-scan".to_string())
            .spawn(move || {
                let mut found = if designs.is_empty() {
                    Vec::new()
                } else {
                    job(&designs)
                };
                if !trees.is_empty() {
                    found.extend(tree_job(&trees));
                }
                // The receiver is gone only when the app is shutting down.
                let _ = sender.send(found);
            });
        match spawned {
            Ok(_) => {
                self.in_flight = Some(InFlight {
                    serial,
                    started: Instant::now(),
                    walks_trees,
                    results,
                    roots,
                    slow_reported: false,
                });
            }
            Err(error) => {
                self.completed_serial = serial;
                self.last_finished = Some(Instant::now());
                self.mark_all(&roots, ScanStatus::Unreadable(error.kind()));
            }
        }
    }

    fn mark_all(&mut self, roots: &[PathBuf], status: ScanStatus) {
        for root in roots {
            self.status.insert(root.clone(), status);
        }
        self.version += 1;
    }
}

#[cfg(test)]
#[path = "projects_host_scheduler_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "projects_host_scheduler_tree_tests.rs"]
mod tree_tests;
