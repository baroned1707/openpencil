//! The disk side of an opened project: every `*.op` file anywhere inside the
//! folder, found by one bounded recursive walk.
//!
//! Like `projects_host_scan.rs` this runs on the scan worker, never on a
//! frame, and never leaves the folder it was given:
//!
//! * directories named in [`SKIPPED_DIRECTORIES`] and every hidden directory
//!   (leading `.`) are not entered, hidden files are not listed;
//! * a symlinked DIRECTORY is never followed (it would only list the same files
//!   twice, or leave the folder), which also rules out loops; a symlinked `.op`
//!   FILE is listed only when its canonical target is a file inside the
//!   canonical root;
//! * the walk is bounded four ways ([`TreeLimits`]): depth, listed files, entries
//!   examined and a time budget. A run that hits the file cap lists the first
//!   files by relative path and reports how many were left out
//!   (`ScanStatus::Truncated`); the files directly inside directories beyond the
//!   depth cap count as left out too. A run that hits the entry or time limit
//!   returns what it found with `ScanStatus::Slow`.

use std::collections::BinaryHeap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::scan::{DesignListing, ScanStatus};

/// Directories that hold dependencies, build output or tool state, never
/// designs. Compared to the directory's own name.
pub(crate) const SKIPPED_DIRECTORIES: [&str; 12] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".venv",
    "venv",
    "Pods",
    "DerivedData",
    "vendor",
];

/// How far and how long one walk may go.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TreeLimits {
    /// Directories deeper than this below the root are not entered.
    pub max_depth: usize,
    /// Files listed; the rest are reported as omitted.
    pub max_files: usize,
    /// Directory entries looked at before the walk gives up.
    pub max_entries: usize,
    /// Wall-clock time after which the walk returns what it has.
    pub budget: Duration,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            max_depth: 12,
            max_files: 2000,
            max_entries: 200_000,
            budget: Duration::from_secs(10),
        }
    }
}

/// How often (in entries) the clock is read.
const CLOCK_EVERY: usize = 256;

/// Every `.op` file under `root` (canonical), sorted by path relative to it.
pub(crate) fn scan_project_tree(root: &Path, limits: &TreeLimits) -> DesignListing {
    let started = Instant::now();
    let canonical_root = match std::fs::canonicalize(root) {
        Ok(path) => path,
        Err(error) => return unreadable(error.kind()),
    };
    let mut walk = Walk {
        limits: *limits,
        canonical_root,
        kept: BinaryHeap::new(),
        found: 0,
        beyond_depth: 0,
        entries: 0,
        cut_short: false,
    };
    let mut pending = vec![(walk.canonical_root.clone(), 0usize)];
    let mut first = true;
    while let Some((dir, depth)) = pending.pop() {
        if walk.out_of_budget(started) {
            break;
        }
        let names = match read_sorted(&dir) {
            Ok(entries) => entries,
            // The root itself unreadable is the project's problem; a subfolder
            // that cannot be read is just skipped.
            Err(error) if first => return unreadable(error.kind()),
            Err(_) => continue,
        };
        first = false;
        for entry in names {
            walk.entries += 1;
            if walk.entries.is_multiple_of(CLOCK_EVERY) && walk.out_of_budget(started) {
                break;
            }
            walk.visit(&entry, depth, &mut pending);
        }
    }
    walk.finish(root)
}

fn unreadable(kind: ErrorKind) -> DesignListing {
    DesignListing {
        files: Vec::new(),
        status: ScanStatus::Unreadable(kind),
    }
}

/// The entries of `dir` in name order, so a cut-short walk is repeatable.
fn read_sorted(dir: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

struct Walk {
    limits: TreeLimits,
    canonical_root: PathBuf,
    /// A max-heap of the smallest `max_files` relative paths seen so far.
    kept: BinaryHeap<PathBuf>,
    /// Every `.op` file seen, kept or not.
    found: usize,
    /// `.op` files directly inside directories beyond the depth cap.
    beyond_depth: usize,
    entries: usize,
    cut_short: bool,
}

impl Walk {
    fn out_of_budget(&mut self, started: Instant) -> bool {
        if self.entries >= self.limits.max_entries || started.elapsed() >= self.limits.budget {
            self.cut_short = true;
        }
        self.cut_short
    }

    fn visit(
        &mut self,
        entry: &std::fs::DirEntry,
        depth: usize,
        pending: &mut Vec<(PathBuf, usize)>,
    ) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            // Hidden files and hidden directories alike.
            return;
        }
        let Ok(file_type) = entry.file_type() else {
            return;
        };
        if file_type.is_dir() {
            if SKIPPED_DIRECTORIES.contains(&&*name) {
                return;
            }
            if depth + 1 > self.limits.max_depth {
                self.beyond_depth += count_op_files(&entry.path());
            } else {
                pending.push((entry.path(), depth + 1));
            }
        } else if is_op(&name) {
            let path = entry.path();
            let usable =
                file_type.is_file() || (file_type.is_symlink() && self.link_to_file_inside(&path));
            if usable {
                if let Ok(relative) = path.strip_prefix(&self.canonical_root) {
                    self.found += 1;
                    let relative = relative.to_path_buf();
                    self.keep(relative);
                }
            }
        }
    }

    fn keep(&mut self, relative: PathBuf) {
        self.kept.push(relative);
        if self.kept.len() > self.limits.max_files {
            self.kept.pop();
        }
    }

    fn link_to_file_inside(&self, link: &Path) -> bool {
        std::fs::canonicalize(link)
            .is_ok_and(|resolved| resolved.starts_with(&self.canonical_root) && resolved.is_file())
    }

    fn finish(self, root: &Path) -> DesignListing {
        let omitted = self.found - self.kept.len() + self.beyond_depth;
        let files = self
            .kept
            .into_sorted_vec()
            .into_iter()
            .map(|relative| root.join(relative))
            .collect();
        let status = if self.cut_short {
            ScanStatus::Slow
        } else if omitted > 0 {
            ScanStatus::Truncated(omitted)
        } else {
            ScanStatus::Ok
        };
        DesignListing { files, status }
    }
}

fn is_op(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension == "op")
}

/// `.op` files directly inside `dir` (no recursion).
fn count_op_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    !name.starts_with('.')
                        && is_op(&name)
                        && entry.file_type().is_ok_and(|kind| kind.is_file())
                })
                .count()
        })
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "projects_host_scan_tree_tests.rs"]
mod tests;
