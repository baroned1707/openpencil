//! The disk side of the Projects dock: list `<root>/designs/*.op` safely.
//!
//! This runs on a worker thread (see `projects_host_scheduler.rs`), never on
//! the frame. It looks at exactly one directory level, never recurses, and
//! never follows a link out of the project: a symlinked `designs` folder or a
//! symlinked `.op` file is used only when it resolves inside the project root.

use std::collections::BinaryHeap;
use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Files listed per project; the rest are reported as omitted.
pub(crate) const MAX_LISTED_FILES: usize = 500;
/// Directory entries looked at before the scan gives up on a huge folder.
const MAX_ENTRIES_EXAMINED: usize = 50_000;

/// How trustworthy one project's listing is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanStatus {
    /// The folder was read in full (or there is no `designs/` folder).
    Ok,
    /// The last scan did not finish in time; the listing is the previous one.
    Slow,
    /// The folder exists but could not be read (permission, I/O error).
    Unreadable(ErrorKind),
    /// More files than fit; this many were left out of the listing.
    Truncated(usize),
}

impl fmt::Display for ScanStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Slow => write!(f, "slow folder, showing the previous listing"),
            Self::Unreadable(kind) => write!(f, "unreadable: {kind}"),
            Self::Truncated(omitted) => write!(f, "{omitted} more files not listed"),
        }
    }
}

/// What a scan found in one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignListing {
    /// At most [`MAX_LISTED_FILES`] paths, sorted.
    pub files: Vec<PathBuf>,
    pub status: ScanStatus,
}

impl DesignListing {
    fn empty(status: ScanStatus) -> Self {
        Self {
            files: Vec::new(),
            status,
        }
    }
}

/// What `<root>/designs` is, as far as this module may use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DesignsDir {
    /// Nothing usable there (absent, or not a folder).
    Missing,
    /// A folder inside the project.
    Ready,
    /// A link that leads out of the project, or a broken or looping link.
    Outside,
    /// The entry cannot be inspected.
    Broken(ErrorKind),
}

/// Classify `<root>/designs` without following a link out of `root`.
pub(crate) fn inspect_designs_dir(root: &Path) -> DesignsDir {
    let designs = root.join("designs");
    let meta = match std::fs::symlink_metadata(&designs) {
        Ok(meta) => meta,
        Err(error) if error.kind() == ErrorKind::NotFound => return DesignsDir::Missing,
        Err(error) => return DesignsDir::Broken(error.kind()),
    };
    if !meta.file_type().is_symlink() {
        return if meta.is_dir() {
            DesignsDir::Ready
        } else {
            DesignsDir::Missing
        };
    }
    match (std::fs::canonicalize(&designs), std::fs::canonicalize(root)) {
        (Ok(resolved), Ok(canonical_root))
            if resolved.starts_with(&canonical_root) && resolved.is_dir() =>
        {
            DesignsDir::Ready
        }
        _ => DesignsDir::Outside,
    }
}

/// `<root>/designs/*.op`, sorted, capped and safe to show. Hidden names and
/// anything that is not a regular `.op` file (or a link to one inside the
/// project) are skipped.
pub(crate) fn scan_design_dir(root: &Path) -> DesignListing {
    match inspect_designs_dir(root) {
        DesignsDir::Missing | DesignsDir::Outside => return DesignListing::empty(ScanStatus::Ok),
        DesignsDir::Broken(kind) => return DesignListing::empty(ScanStatus::Unreadable(kind)),
        DesignsDir::Ready => {}
    }
    let designs = root.join("designs");
    let entries = match std::fs::read_dir(&designs) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return DesignListing::empty(ScanStatus::Ok)
        }
        Err(error) => return DesignListing::empty(ScanStatus::Unreadable(error.kind())),
    };
    let mut canonical_root: Option<Option<PathBuf>> = None;
    // A max-heap of the smallest `MAX_LISTED_FILES` paths seen so far.
    let mut kept: BinaryHeap<PathBuf> = BinaryHeap::new();
    let mut found = 0usize;
    for entry in entries.take(MAX_ENTRIES_EXAMINED) {
        let Ok(entry) = entry else { continue };
        // The name decides first: no system call for the many non-`.op` names.
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text.starts_with('.')
            || Path::new(&*name_text).extension().is_none_or(|e| e != "op")
        {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let usable = if file_type.is_file() {
            true
        } else if file_type.is_symlink() {
            let root_resolved = canonical_root
                .get_or_insert_with(|| std::fs::canonicalize(root).ok())
                .as_deref();
            link_to_file_inside(&entry.path(), root_resolved)
        } else {
            false
        };
        if !usable {
            continue;
        }
        found += 1;
        kept.push(designs.join(&name));
        if kept.len() > MAX_LISTED_FILES {
            kept.pop();
        }
    }
    let files = kept.into_sorted_vec();
    let status = if found > files.len() {
        ScanStatus::Truncated(found - files.len())
    } else {
        ScanStatus::Ok
    };
    DesignListing { files, status }
}

/// Whether the symlink at `link` resolves to a regular file inside the project.
/// A loop or a dangling link fails to canonicalize and is skipped.
fn link_to_file_inside(link: &Path, canonical_root: Option<&Path>) -> bool {
    let Some(canonical_root) = canonical_root else {
        return false;
    };
    std::fs::canonicalize(link)
        .is_ok_and(|resolved| resolved.starts_with(canonical_root) && resolved.is_file())
}

/// `<root>/designs/*.op`, sorted by name; an absent or unreadable folder gives
/// an empty list. The status-carrying form is [`scan_design_dir`].
#[cfg(test)]
pub(crate) fn list_design_files(root: &Path) -> Vec<PathBuf> {
    scan_design_dir(root).files
}

#[cfg(test)]
#[path = "projects_host_scan_tests.rs"]
mod tests;
