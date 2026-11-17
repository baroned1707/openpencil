//! The file-system half of renaming a design from the Projects dock.
//!
//! [`plan_rename`] looks at the disk and decides, from the request alone and
//! without trusting the widget, whether the rename may happen: the name obeys
//! the rules, the source is a regular `.op` file (not a link) whose real
//! location is inside a project the dock shows, and no OTHER file in its folder
//! already has the target name (compared ignoring case, because the default
//! macOS volume does). [`execute_rename`] then renames inside the one folder.
//!
//! Never overwrites. The rename uses the operating system's no-replace
//! primitive where there is one (macOS `renamex_np` with `RENAME_EXCL`, Linux
//! `renameat2` with `RENAME_NOREPLACE`), so a file that appears between the
//! check and the rename is still not replaced. On any other system, and on a
//! volume that lacks the primitive, it falls back to check-then-rename and the
//! window between the two calls is a real (tiny) race: a file created in it
//! would be replaced. Nothing the editor does creates such a file on its own.
//!
//! A change of case only (`Card.op` to `card.op`) names the same file on a
//! case-insensitive volume, so the no-replace call would refuse it; it is a
//! plain rename, safe because the check above found no other entry that
//! matches the target ignoring case.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use op_editor_core::project_rename::normalized_stem;
use op_editor_core::RenameNameError;

/// Why a rename did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenameRefusal {
    /// The name breaks a rule (the widget checks too; this is the real gate).
    InvalidName(RenameNameError),
    /// The source is not a regular `.op` file (a link, a folder, another type).
    NotADesignFile,
    /// The source is not inside any project the dock shows.
    OutsideProjects,
    /// The source no longer exists.
    SourceGone,
    /// Another file in the folder has the target name.
    NameTaken,
    /// The file is open in more than one tab.
    OpenInSeveralTabs,
    /// A save is being written; renaming now could let it recreate the old name.
    SaveInProgress,
    /// A `.fig` conversion is writing this file.
    ConversionRunning,
    /// The operating system refused (permission, read-only volume, ...).
    Io(String),
}

impl fmt::Display for RenameRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(_) => write!(f, "that name cannot be used"),
            Self::NotADesignFile => write!(f, "not a regular .op file"),
            Self::OutsideProjects => write!(f, "the file is outside the open projects"),
            Self::SourceGone => write!(f, "the file no longer exists"),
            Self::NameTaken => write!(f, "a file with that name already exists"),
            Self::OpenInSeveralTabs => write!(f, "the file is open in more than one tab"),
            Self::SaveInProgress => write!(f, "a save is being written"),
            Self::ConversionRunning => write!(f, "a conversion is writing this file"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for RenameRefusal {}

/// A rename that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenamePlan {
    /// The file, as listed.
    pub source: PathBuf,
    /// The new path: the same folder, the new stem, the file's own extension.
    pub target: PathBuf,
    /// Only the case of the name changes.
    pub case_only: bool,
}

/// A design file that passed the checks every action on a listed row needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesignLocation {
    /// The file's folder, canonical.
    pub real_parent: PathBuf,
    /// The file's own extension (`op` in any case).
    pub extension: String,
}

/// The checks shared by every action on a listed `.op` row (rename, move to the
/// Trash): `source` exists, is a regular file (not a link of any kind, so it
/// cannot point out of the project), has the `.op` extension, and its real
/// folder lies inside one of `roots` (the project roots the dock shows). The
/// widget's own checks are never trusted.
pub(crate) fn locate_design_file(
    roots: &[PathBuf],
    source: &Path,
) -> Result<DesignLocation, RenameRefusal> {
    let meta = std::fs::symlink_metadata(source).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => RenameRefusal::SourceGone,
        _ => RenameRefusal::Io(error.to_string()),
    })?;
    if !meta.file_type().is_file() {
        return Err(RenameRefusal::NotADesignFile);
    }
    let extension = match source.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("op") => extension.to_string(),
        _ => return Err(RenameRefusal::NotADesignFile),
    };
    let Some(parent) = source.parent() else {
        return Err(RenameRefusal::NotADesignFile);
    };
    let real_parent = std::fs::canonicalize(parent).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => RenameRefusal::SourceGone,
        _ => RenameRefusal::Io(error.to_string()),
    })?;
    let inside = roots
        .iter()
        .any(|root| std::fs::canonicalize(root).is_ok_and(|root| real_parent.starts_with(root)));
    if !inside {
        return Err(RenameRefusal::OutsideProjects);
    }
    Ok(DesignLocation {
        real_parent,
        extension,
    })
}

/// Check a rename of `source` to `raw_stem` against `roots` (the project roots
/// the dock shows) and the disk. `Ok(None)` means the name is the file's own:
/// nothing to do.
pub(crate) fn plan_rename(
    roots: &[PathBuf],
    source: &Path,
    raw_stem: &str,
) -> Result<Option<RenamePlan>, RenameRefusal> {
    let stem = normalized_stem(raw_stem).map_err(RenameRefusal::InvalidName)?;
    let DesignLocation {
        real_parent,
        extension,
    } = locate_design_file(roots, source)?;
    let (Some(parent), Some(file_name)) = (source.parent(), source.file_name()) else {
        return Err(RenameRefusal::NotADesignFile);
    };
    let current = file_name.to_string_lossy().into_owned();
    let wanted = format!("{stem}.{extension}");
    if wanted == current {
        return Ok(None);
    }
    let wanted_lower = wanted.to_lowercase();
    let entries =
        std::fs::read_dir(&real_parent).map_err(|error| RenameRefusal::Io(error.to_string()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != current && name.to_lowercase() == wanted_lower {
            return Err(RenameRefusal::NameTaken);
        }
    }
    Ok(Some(RenamePlan {
        source: source.to_path_buf(),
        target: parent.join(&wanted),
        case_only: current.to_lowercase() == wanted_lower,
    }))
}

/// Rename `plan.source` to `plan.target` without replacing anything.
pub(crate) fn execute_rename(plan: &RenamePlan) -> Result<(), RenameRefusal> {
    let result = if plan.case_only {
        std::fs::rename(&plan.source, &plan.target)
    } else {
        rename_no_replace(&plan.source, &plan.target)
    };
    result.map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => RenameRefusal::SourceGone,
        io::ErrorKind::AlreadyExists => RenameRefusal::NameTaken,
        _ => RenameRefusal::Io(error.to_string()),
    })
}

/// Rename `from` to `to`, failing with `AlreadyExists` instead of replacing.
pub(crate) fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match no_replace_call(from, to) {
            // The volume has no such primitive: fall back below.
            Err(error) if is_unsupported(&error) => {}
            outcome => return outcome,
        }
    }
    check_then_rename(from, to)
}

/// The fallback: look, then rename. A file created between the two calls would
/// be replaced; see the module docs.
fn check_then_rename(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    std::fs::rename(from, to)
}

/// The volume (or kernel) lacks the no-replace flag. macOS answers ENOTSUP;
/// Linux answers EINVAL (the filesystem) or ENOSYS (an old kernel).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn is_unsupported(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    if cfg!(target_os = "macos") {
        code == libc::ENOTSUP
    } else {
        code == libc::EINVAL || code == libc::ENOSYS
    }
}

#[cfg(target_os = "macos")]
fn no_replace_call(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = c_paths(from, to)?;
    // SAFETY: both pointers are NUL-terminated strings that outlive the call.
    let status = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn no_replace_call(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = c_paths(from, to)?;
    // SAFETY: both pointers are NUL-terminated strings that outlive the call;
    // `renameat2` has no libc wrapper on every target, so it is the raw syscall.
    let status = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn c_paths(from: &Path, to: &Path) -> io::Result<(std::ffi::CString, std::ffi::CString)> {
    use std::os::unix::ffi::OsStrExt;
    let convert = |path: &Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    };
    Ok((convert(from)?, convert(to)?))
}

#[cfg(test)]
#[path = "projects_host_rename_fs_tests.rs"]
mod tests;
