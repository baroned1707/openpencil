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

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_host_services::project_root::ProjectRootCache;

use super::scan::{scan_design_dir, DesignListing, ScanStatus};

/// How often `<root>/designs` is re-read while nothing else changed, so a file
/// an agent created from outside the editor shows up.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(3);
/// A scan still running after this long is abandoned (marked slow).
pub(crate) const SLOW_AFTER: Duration = Duration::from_secs(5);

/// What one scan does with a set of project roots. Replaceable so tests can
/// block, count and time it.
pub(crate) type ScanJob = Arc<dyn Fn(&[PathBuf]) -> Vec<(PathBuf, DesignListing)> + Send + Sync>;

/// The real job: re-resolve project roots that went stale (off the UI thread),
/// then list every project's `designs/` folder.
pub(crate) fn disk_scan_job(roots: Arc<ProjectRootCache>) -> ScanJob {
    Arc::new(move |project_roots| {
        roots.refresh_stale();
        project_roots
            .iter()
            .map(|root| (root.clone(), scan_design_dir(root)))
            .collect()
    })
}

struct InFlight {
    started: Instant,
    results: Receiver<Vec<(PathBuf, DesignListing)>>,
    roots: Vec<PathBuf>,
    slow_reported: bool,
}

pub(crate) struct ScanScheduler {
    job: ScanJob,
    every: Duration,
    slow_after: Duration,
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
            job,
            every,
            slow_after,
            in_flight: None,
            wanted: true,
            last_finished: None,
            files: BTreeMap::new(),
            status: BTreeMap::new(),
            version: 0,
        }
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
                self.start(roots.iter().cloned().collect());
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
                if flight.slow_reported || flight.started.elapsed() < self.slow_after {
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
                self.in_flight = None;
                self.last_finished = Some(Instant::now());
                self.mark_all(&roots, ScanStatus::Unreadable(ErrorKind::Other));
                true
            }
        }
    }

    fn start(&mut self, roots: Vec<PathBuf>) {
        self.wanted = false;
        let (sender, results) = mpsc::channel();
        let job = Arc::clone(&self.job);
        let scan_roots = roots.clone();
        let spawned = std::thread::Builder::new()
            .name("projects-scan".to_string())
            .spawn(move || {
                // The receiver is gone only when the app is shutting down.
                let _ = sender.send(job(&scan_roots));
            });
        match spawned {
            Ok(_) => {
                self.in_flight = Some(InFlight {
                    started: Instant::now(),
                    results,
                    roots,
                    slow_reported: false,
                });
            }
            Err(error) => {
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
