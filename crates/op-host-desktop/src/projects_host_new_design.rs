//! Creating `<root>/designs/untitled[-n].op` for the Projects dock's `+`.
//!
//! The file is created with `save_new_to_path`, which refuses to replace an
//! existing entry, so a name taken between looking and writing (an agent
//! created it, or another window did) makes it retry with the next name
//! instead of failing or overwriting. A failure leaves nothing behind: no
//! half-written file, and no `designs/` folder this call created.

use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use op_host_services::doc_io::SaveNewError;

use super::scan::{inspect_designs_dir, DesignsDir};

/// Names tried before giving up (each attempt moves to the next free number).
pub(crate) const MAX_NAME_ATTEMPTS: usize = 20;

/// Why a new design could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NewDesignError {
    /// The `designs/` folder could not be created or inspected.
    CreateFolder(ErrorKind),
    /// `designs/` is a link that leads out of the project.
    FolderOutsideProject,
    /// The file could not be written.
    Write(SaveNewError),
    /// Every name tried was taken at the moment of writing.
    NamesTaken,
}

impl fmt::Display for NewDesignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateFolder(kind) => write!(f, "cannot create the designs folder: {kind}"),
            Self::FolderOutsideProject => {
                write!(f, "the designs folder points outside the project")
            }
            Self::Write(SaveNewError::AlreadyExists) => write!(f, "the name is already taken"),
            Self::Write(SaveNewError::Io(error)) => write!(f, "{error}"),
            Self::NamesTaken => write!(f, "no free name after {MAX_NAME_ATTEMPTS} attempts"),
        }
    }
}

impl std::error::Error for NewDesignError {}

fn design_path(designs: &Path, number: usize) -> PathBuf {
    if number <= 1 {
        designs.join("untitled.op")
    } else {
        designs.join(format!("untitled-{number}.op"))
    }
}

/// The path the next new design in `root` would get: `untitled.op`, then
/// `untitled-2.op`, ...
#[cfg(test)]
pub(crate) fn next_untitled_design(root: &Path) -> PathBuf {
    let designs = root.join("designs");
    let number = first_free_number(&designs);
    design_path(&designs, number)
}

fn first_free_number(designs: &Path) -> usize {
    (1..)
        .find(|&number| std::fs::symlink_metadata(design_path(designs, number)).is_err())
        .expect("an unused name exists")
}

/// Create the folder (if needed) and a starter design in `root`, writing each
/// candidate with `write`. Returns the path that was created.
pub(crate) fn create_design_file(
    root: &Path,
    write: &dyn Fn(&Path) -> Result<(), SaveNewError>,
) -> Result<PathBuf, NewDesignError> {
    let designs = root.join("designs");
    let made_folder = match inspect_designs_dir(root) {
        DesignsDir::Outside => return Err(NewDesignError::FolderOutsideProject),
        DesignsDir::Broken(kind) => return Err(NewDesignError::CreateFolder(kind)),
        DesignsDir::Ready => false,
        DesignsDir::Missing => {
            std::fs::create_dir_all(&designs)
                .map_err(|error| NewDesignError::CreateFolder(error.kind()))?;
            true
        }
    };
    let outcome = write_first_free(&designs, write);
    if outcome.is_err() && made_folder {
        // Succeeds only while the folder is empty, so a design that appeared
        // there meanwhile is never touched.
        let _ = std::fs::remove_dir(&designs);
    }
    outcome
}

fn write_first_free(
    designs: &Path,
    write: &dyn Fn(&Path) -> Result<(), SaveNewError>,
) -> Result<PathBuf, NewDesignError> {
    let mut number = first_free_number(designs);
    for _ in 0..MAX_NAME_ATTEMPTS {
        let target = design_path(designs, number);
        match write(&target) {
            Ok(()) => return Ok(target),
            Err(SaveNewError::AlreadyExists) => number += 1,
            Err(error) => return Err(NewDesignError::Write(error)),
        }
    }
    Err(NewDesignError::NamesTaken)
}

#[cfg(test)]
#[path = "projects_host_new_design_tests.rs"]
mod tests;
