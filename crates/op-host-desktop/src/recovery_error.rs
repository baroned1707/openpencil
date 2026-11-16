//! Typed failure of the crash-recovery store (`recovery_store.rs`).
//!
//! Style follows `save_session::SaveError`: a plain enum plus a hand-written
//! `Display`, no `thiserror`. Every variant names the file or directory it
//! concerns so a line in the log is enough to find the entry. Nothing here is
//! ever shown as a modal: a failed snapshot is logged and retried.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecoveryError {
    /// The recovery directory could not be created or listed.
    Directory { path: String, detail: String },
    /// The canonical `.op` snapshot could not be written.
    Document { path: String, detail: String },
    /// The manifest could not be serialized, written or replaced.
    Manifest { path: String, detail: String },
    /// A snapshot could not be taken from the live document (a collaboration
    /// session refused to lend a parked document, or the worker is gone).
    Capture { detail: String },
    /// An entry could not be removed.
    Remove { path: String, detail: String },
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory { path, detail } => {
                write!(f, "recovery directory {path}: {detail}")
            }
            Self::Document { path, detail } => {
                write!(f, "recovery snapshot {path}: {detail}")
            }
            Self::Manifest { path, detail } => {
                write!(f, "recovery manifest {path}: {detail}")
            }
            Self::Capture { detail } => write!(f, "recovery capture: {detail}"),
            Self::Remove { path, detail } => {
                write!(f, "recovery entry {path} could not be removed: {detail}")
            }
        }
    }
}

impl std::error::Error for RecoveryError {}
