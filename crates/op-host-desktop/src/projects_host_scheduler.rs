//! Runs the Projects dock's disk scan off the UI thread.
//!
//! The frame never reads a directory. Each frame it calls [`ScanScheduler::poll`],
//! which (i) drains a finished scan from the channel and (ii) starts a new one
//! on a fresh thread when one is due: when something that can change the
//! listing happened ([`ScanScheduler::request_now`]) or every few seconds while
//! the caller says a background refresh is wanted (the window is focused and the
//! dock is showing). At most one scan is ever alive.
//!
//! A scan that has not finished after `slow_after` is *abandoned*, not killed
//! (a `read_dir` stuck on a dead network drive cannot be interrupted): its
//! projects are marked [`ScanStatus::Slow`] and keep their last listing, and no
//! new scan starts until the stuck one returns. When it does return late, its
//! result is applied like any other, so the listing catches up and `Slow`
//! clears. A request that arrives meanwhile is remembered and runs next.
//!
//! Two kinds of root share the one worker. An inferred project's `designs/` folder
//! is cheap. A folder the user opened is walked recursively
//! (`projects_host_scan_tree.rs`) when it was just opened or refreshed
//! ([`ScanScheduler::request_tree_scan`]), and AGAIN whenever the app asks for a
//! full rescan: the window regained focus, an in-app file operation finished
//! ([`ScanScheduler::request_rescan_all`]) or the background tick came due. The
//! tick runs only while the caller's `active` check passes, waits at least
//! three times as long as the last scan took (a huge repository is not walked
//! back to back) and an unchanged result changes nothing, so the dock does not
//! flicker. Each started scan gets a serial number, so the host can tell "the
//! scan I asked for has finished" from "some older scan did".
//!
//! Besides the listings, every scan stats the paths of the open documents
//! ([`ScanScheduler::set_watched`]) and reports the ones that are gone, so the
//! dock can mark an open tab whose file vanished. That is the only way to know:
//! an open file missing from a listing may just be outside what the listing
//! covers (a hidden folder, `vendor/`, deeper than the walk goes).

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_host_services::project_root::ProjectRootCache;

use super::scan::{scan_design_dir, DesignListing, ScanStatus};
use super::scan_tree::{scan_project_tree, TreeLimits};

/// How often the listings are refreshed in the background while the window is
/// focused and the dock is showing, so a file created, renamed or deleted
/// outside the editor shows up.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(5);
/// A background rescan waits at least this many times as long as the previous
/// scan took.
const BACKOFF_FACTOR: u32 = 3;
/// A scan still running after this long is abandoned (marked slow).
pub(crate) const SLOW_AFTER: Duration = Duration::from_secs(5);
/// A scan that includes a recursive walk is abandoned only after this long:
/// the walk has a time budget of its own (10 s) and returns its partial result
/// by then, so this fires only when the walk is stuck inside a system call.
pub(crate) const TREE_SLOW_AFTER: Duration = Duration::from_secs(12);

/// What one scan does with a set of project roots. Replaceable so tests can
/// block, count and time it.
pub(crate) type ScanJob = Arc<dyn Fn(&[PathBuf]) -> Vec<(PathBuf, DesignListing)> + Send + Sync>;

/// What a finished scan hands back.
struct ScanOutput {
    listings: Vec<(PathBuf, DesignListing)>,
    /// The watched paths that no longer exist.
    missing: BTreeSet<PathBuf>,
}

/// A change the app made to the disk while a scan was running. That scan read
/// the folder before the change, so its result is patched the same way when it
/// lands; otherwise the row would flicker back (or fail to appear) until the
/// next scan.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ListingPatch {
    Rename(PathBuf, PathBuf),
    /// A file created in the listing of `root`.
    Add {
        root: PathBuf,
        file: PathBuf,
    },
    Remove(PathBuf),
}

impl ListingPatch {
    /// Apply to the listing of `root`; whether it changed.
    fn apply(&self, root: &Path, files: &mut Vec<PathBuf>) -> bool {
        match self {
            Self::Rename(old, new) => swap_listed(files, old, new),
            Self::Add { root: target, file } => target == root && insert_listed_sorted(files, file),
            Self::Remove(path) => {
                let before = files.len();
                files.retain(|listed| listed != path);
                files.len() != before
            }
        }
    }
}

/// The watched paths that are certainly gone (not found; a permission error is
/// not "gone").
fn missing_paths(watched: &[PathBuf]) -> BTreeSet<PathBuf> {
    watched
        .iter()
        .filter(|path| {
            std::fs::symlink_metadata(path).is_err_and(|error| error.kind() == ErrorKind::NotFound)
        })
        .cloned()
        .collect()
}

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
    results: Receiver<ScanOutput>,
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
    /// The next scan walks every opened root, not only the dirty ones.
    rescan_all: bool,
    /// Paths of the open documents; each scan checks that they still exist.
    watched: Vec<PathBuf>,
    /// The watched paths the last scan did not find.
    missing: BTreeSet<PathBuf>,
    /// How long the last scan took.
    last_duration: Duration,
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
    /// Changes made while a scan was already running, with the serial of that
    /// scan (see [`ListingPatch`]).
    patches: Vec<(ListingPatch, u64)>,
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
            rescan_all: false,
            watched: Vec::new(),
            missing: BTreeSet::new(),
            last_duration: Duration::ZERO,
            started_serial: 0,
            completed_serial: 0,
            in_flight: None,
            wanted: true,
            last_finished: None,
            files: BTreeMap::new(),
            status: BTreeMap::new(),
            version: 0,
            patches: Vec::new(),
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

    /// A listed file was renamed on disk: show the new name at once, in every
    /// listing that holds the old one, instead of waiting for the next scan.
    pub(crate) fn rename_listed(&mut self, old: &Path, new: &Path) {
        let patch = ListingPatch::Rename(old.to_path_buf(), new.to_path_buf());
        self.patch_listed(patch);
    }

    /// A file was created inside `root`'s listing: show it at once.
    pub(crate) fn insert_listed(&mut self, root: &Path, file: &Path) {
        let patch = ListingPatch::Add {
            root: root.to_path_buf(),
            file: file.to_path_buf(),
        };
        self.patch_listed(patch);
    }

    /// A listed file left the disk (moved to the Trash): drop its row at once.
    pub(crate) fn remove_listed(&mut self, file: &Path) {
        self.patch_listed(ListingPatch::Remove(file.to_path_buf()));
    }

    fn patch_listed(&mut self, patch: ListingPatch) {
        let mut changed = false;
        for (root, files) in &mut self.files {
            changed |= patch.apply(root, files);
        }
        if let Some(flight) = self.in_flight.as_ref() {
            self.patches.push((patch, flight.serial));
        }
        if changed {
            self.version += 1;
        }
    }

    /// Walk every opened project (and re-list the inferred ones) at the next
    /// chance: the window regained focus, or an in-app file operation finished.
    pub(crate) fn request_rescan_all(&mut self) {
        self.rescan_all = true;
        self.wanted = true;
    }

    /// The open documents' paths, checked by every scan. A change asks for a
    /// scan so a tab opened on a file that is already gone is marked at once.
    pub(crate) fn set_watched(&mut self, paths: Vec<PathBuf>) {
        if self.watched != paths {
            self.watched = paths;
            self.wanted = true;
        }
    }

    /// Whether any listing holds `path`.
    pub(crate) fn is_listed(&self, path: &Path) -> bool {
        self.files
            .values()
            .any(|files| files.iter().any(|listed| listed == path))
    }

    /// The watched paths the last scan did not find.
    pub(crate) fn missing(&self) -> &BTreeSet<PathBuf> {
        &self.missing
    }

    /// When the next background rescan is due, or `None` while a scan runs or
    /// one is already wanted (the next frame starts it).
    pub(crate) fn next_background_due(&self) -> Option<Instant> {
        if self.in_flight.is_some() || self.wanted {
            return None;
        }
        Some(
            self.last_finished
                .map_or_else(Instant::now, |at| at + self.background_interval()),
        )
    }

    fn background_interval(&self) -> Duration {
        self.every.max(self.last_duration * BACKOFF_FACTOR)
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

    /// One frame's worth of scan bookkeeping. Never blocks. `active` is asked
    /// only when a background rescan is due: it says whether one is wanted now
    /// (the window is focused and the dock is showing); when it is not, an idle
    /// app does no file-system work at all. Returns whether the listing or a
    /// status changed.
    pub(crate) fn poll(
        &mut self,
        roots: &BTreeSet<PathBuf>,
        active: impl FnOnce() -> bool,
    ) -> bool {
        let changed = self.drain();
        if self.in_flight.is_none() && !roots.is_empty() {
            let background_due = self
                .last_finished
                .is_none_or(|at| at.elapsed() >= self.background_interval());
            let background = !self.wanted && background_due && active();
            if self.wanted || background {
                let designs: Vec<PathBuf> = roots
                    .iter()
                    .filter(|root| !self.trees.contains(*root))
                    .cloned()
                    .collect();
                let every_tree = background || self.rescan_all;
                let trees: Vec<PathBuf> = if every_tree {
                    &self.trees
                } else {
                    &self.dirty_trees
                }
                .iter()
                .filter(|root| roots.contains(*root))
                .cloned()
                .collect();
                if designs.is_empty() && trees.is_empty() {
                    self.wanted = false;
                    self.rescan_all = false;
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
            Ok(output) => {
                let serial = flight.serial;
                self.completed_serial = serial;
                self.last_duration = flight.started.elapsed();
                self.in_flight = None;
                self.last_finished = Some(Instant::now());
                // Only a difference counts: an unchanged rescan must not bump
                // the version, or the dock would rebuild its rows for nothing.
                let mut changed = false;
                for (root, mut listing) in output.listings {
                    // A change made after this scan started is not in what it read.
                    for (patch, after) in &self.patches {
                        if *after >= serial {
                            patch.apply(&root, &mut listing.files);
                        }
                    }
                    changed |=
                        self.status.insert(root.clone(), listing.status) != Some(listing.status);
                    if self.files.get(&root) != Some(&listing.files) {
                        self.files.insert(root, listing.files);
                        changed = true;
                    }
                }
                if self.missing != output.missing {
                    self.missing = output.missing;
                    changed = true;
                }
                self.patches.retain(|(_, after)| *after > serial);
                if changed {
                    self.version += 1;
                }
                changed
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
                let mut changed = false;
                for root in &flight.roots {
                    changed |= self.status.insert(root.clone(), ScanStatus::Slow)
                        != Some(ScanStatus::Slow);
                }
                if changed {
                    self.version += 1;
                }
                changed
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
        self.rescan_all = false;
        self.dirty_trees.clear();
        self.started_serial += 1;
        let serial = self.started_serial;
        let roots: Vec<PathBuf> = designs.iter().chain(&trees).cloned().collect();
        let (sender, results) = mpsc::channel();
        let job = Arc::clone(&self.job);
        let tree_job = Arc::clone(&self.tree_job);
        let walks_trees = !trees.is_empty();
        let watched = self.watched.clone();
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
                let missing = missing_paths(&watched);
                // The receiver is gone only when the app is shutting down.
                let _ = sender.send(ScanOutput {
                    listings: found,
                    missing,
                });
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

/// Replace `old` with `new` in a listing, unless `new` is already there.
/// Returns whether the listing changed.
fn swap_listed(files: &mut Vec<PathBuf>, old: &Path, new: &Path) -> bool {
    let Some(at) = files.iter().position(|path| path == old) else {
        return false;
    };
    if files.iter().any(|path| path == new) {
        files.remove(at);
    } else {
        files[at] = new.to_path_buf();
    }
    true
}

/// Insert `file` keeping the listing sorted; `false` when it is already there.
fn insert_listed_sorted(files: &mut Vec<PathBuf>, file: &Path) -> bool {
    match files.binary_search_by(|listed| listed.as_path().cmp(file)) {
        Ok(_) => false,
        Err(at) => {
            files.insert(at, file.to_path_buf());
            true
        }
    }
}

#[cfg(test)]
#[path = "projects_host_scheduler_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "projects_host_scheduler_tree_tests.rs"]
mod tree_tests;
