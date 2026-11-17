//! What converting one listed `.fig` file means, decided from the disk at the
//! moment the conversion is about to start (never from the scan that listed it).
//!
//! Three questions are answered here, and only here:
//! * is the file still a regular `.fig` that lies inside the opened project
//!   (a file swapped for a link that leaves the folder after the scan is
//!   refused), and is it small enough to be worth a child process;
//! * does its `<stem>.op` already exist (then there is nothing to convert: the
//!   row is "Converted" and a click opens that file);
//! * where does the output go: next to the `.fig`, under a name that is free.
//!
//! The output is never an existing path. [`unique_output_path`] only proposes
//! names; the child process still creates the file with a no-replace publish, so
//! a name taken in the meantime is reported instead of overwritten.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::convert_fig_cli::MAX_INPUT_BYTES;

/// Names tried for one output before giving up.
pub(crate) const MAX_OUTPUT_NAMES: usize = 1000;

/// What to do with a listed `.fig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Plan {
    /// `<stem>.op` is already there; open it.
    AlreadyConverted(PathBuf),
    /// Run the converter from `source` (canonical) to `output` (a free name next
    /// to it, inside the project).
    Convert { source: PathBuf, output: PathBuf },
}

/// Why a `.fig` is not converted. The text is the short reason a row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The path is not a `.fig` file name.
    NotAFigFile,
    /// The file (or the folder it is in) resolves outside the project root.
    OutsideProject,
    /// It is gone, a folder, a device or another non-regular file.
    NotARegularFile,
    /// Larger than the converter accepts ([`MAX_INPUT_BYTES`]).
    TooLarge,
    /// The file or its folder cannot be inspected.
    Unreadable(std::io::ErrorKind),
    /// Every candidate output name is taken.
    NoFreeName,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAFigFile => write!(f, "not a .fig file"),
            Self::OutsideProject => write!(f, "outside the project"),
            Self::NotARegularFile => write!(f, "not a regular file"),
            Self::TooLarge => write!(f, "too large"),
            Self::Unreadable(kind) => write!(f, "cannot read: {kind}"),
            Self::NoFreeName => write!(f, "no free output name"),
        }
    }
}

/// Decide what converting `listed` (a path the scan produced under `root`, which
/// must be canonical) does, from what is on disk now.
pub(crate) fn plan_conversion(root: &Path, listed: &Path) -> Result<Plan, Refusal> {
    let is_fig = listed
        .extension()
        .is_some_and(|extension| extension == "fig");
    let (Some(stem), true) = (listed.file_stem(), is_fig) else {
        return Err(Refusal::NotAFigFile);
    };
    let source =
        std::fs::canonicalize(listed).map_err(|error| Refusal::Unreadable(error.kind()))?;
    let folder = listed
        .parent()
        .ok_or(Refusal::NotARegularFile)
        .and_then(|parent| {
            std::fs::canonicalize(parent).map_err(|error| Refusal::Unreadable(error.kind()))
        })?;
    if !source.starts_with(root) || !folder.starts_with(root) {
        return Err(Refusal::OutsideProject);
    }
    let meta = std::fs::metadata(&source).map_err(|error| Refusal::Unreadable(error.kind()))?;
    if !meta.is_file() {
        return Err(Refusal::NotARegularFile);
    }
    if meta.len() > MAX_INPUT_BYTES {
        return Err(Refusal::TooLarge);
    }
    let sibling = folder.join(with_extension_suffix(stem, ""));
    if is_openable_op(root, &sibling) {
        return Ok(Plan::AlreadyConverted(sibling));
    }
    let output = unique_output_path(&folder, stem, &|candidate| {
        std::fs::symlink_metadata(candidate).is_ok()
    })
    .ok_or(Refusal::NoFreeName)?;
    Ok(Plan::Convert { source, output })
}

/// Whether `<stem>.op` is a design the dock would list: a regular file, or a
/// link to one inside the project.
fn is_openable_op(root: &Path, path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.is_file() {
        return true;
    }
    meta.file_type().is_symlink()
        && std::fs::canonicalize(path)
            .is_ok_and(|resolved| resolved.starts_with(root) && resolved.is_file())
}

/// `<stem><suffix>.op` as a file name.
fn with_extension_suffix(stem: &std::ffi::OsStr, suffix: &str) -> OsString {
    let mut name = stem.to_os_string();
    name.push(suffix);
    name.push(".op");
    name
}

/// The first free output name in `folder`: `<stem>.op`, then `<stem>-converted.op`,
/// `<stem>-converted-2.op`, ... `taken` says whether a path is occupied (by
/// anything, a dangling link included). `None` after [`MAX_OUTPUT_NAMES`] names.
pub(crate) fn unique_output_path(
    folder: &Path,
    stem: &std::ffi::OsStr,
    taken: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    (0..MAX_OUTPUT_NAMES)
        .map(|attempt| {
            let suffix = match attempt {
                0 => String::new(),
                1 => "-converted".to_string(),
                n => format!("-converted-{n}"),
            };
            folder.join(with_extension_suffix(stem, &suffix))
        })
        .find(|candidate| !taken(candidate))
}

#[cfg(test)]
#[path = "projects_host_fig_plan_tests.rs"]
mod tests;
