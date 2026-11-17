//! Moving a design to the Trash from the Projects dock, on the desktop side.
//!
//! The dock only records a confirmed path (`ProjectsPanelState::trash_commit`).
//! This file executes it and the order is the point: every check runs against
//! the live state and the disk first, the file is handed to the operating
//! system's Trash, and ONLY THEN does anything that remembers the path change.
//! A refusal or a failure leaves the row, the tabs and the disk as they were.
//!
//! It is a recoverable move, never a delete: [`TrashBackend`] has no "delete"
//! operation, so a bug here cannot lose a file for good. A platform without an
//! implementation reports `supported() == false`, the dock hides the action and
//! the host refuses a forged request. macOS uses `NSFileManager`
//! (`projects_host_trash_macos.rs`); Windows and Linux have no backend yet.
//!
//! Refused when the path is not a regular `.op` file inside a shown project
//! (links never qualify), when it is open in any tab (the user closes the tab
//! first), when a save is being written or a `.fig` conversion writes it.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use op_editor_core::editor_toast::EditorToastLevel;

use super::rename_fs::{locate_design_file, RenameRefusal};
use crate::DesktopApp;

/// i18n keys of the toasts a refused or failed move raises.
const TRASH_FAILED_KEY: &str = "projects.trashFailed";
const TRASH_OPEN_TAB_KEY: &str = "projects.trashOpenTab";

/// Where a removed file goes. Replaceable so tests never touch the real Trash.
pub(crate) trait TrashBackend: Send + Sync {
    /// Whether this platform can move a file to the Trash at all.
    fn supported(&self) -> bool;
    /// Move `path` to the Trash. Never deletes.
    fn trash(&self, path: &Path) -> io::Result<()>;
}

/// The operating system's Trash.
pub(crate) struct SystemTrash;

impl TrashBackend for SystemTrash {
    fn supported(&self) -> bool {
        cfg!(target_os = "macos")
    }

    #[cfg(target_os = "macos")]
    fn trash(&self, path: &Path) -> io::Result<()> {
        super::trash_macos::move_to_trash(path)
    }

    #[cfg(not(target_os = "macos"))]
    fn trash(&self, _path: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this platform has no Trash backend",
        ))
    }
}

pub(crate) fn default_backend() -> Arc<dyn TrashBackend> {
    Arc::new(SystemTrash)
}

/// Why a file was not moved to the Trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrashRefusal {
    /// This platform has no Trash backend.
    Unsupported,
    /// Not a regular `.op` file.
    NotADesignFile,
    /// Not inside any project the dock shows.
    OutsideProjects,
    /// The file is already gone.
    SourceGone,
    /// A tab holds the file.
    OpenInTab,
    /// A save is being written.
    SaveInProgress,
    /// A `.fig` conversion is writing this file.
    ConversionRunning,
    /// The operating system refused (permission, volume without a Trash, ...).
    Io(String),
}

impl fmt::Display for TrashRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "this platform has no Trash"),
            Self::NotADesignFile => write!(f, "not a regular .op file"),
            Self::OutsideProjects => write!(f, "the file is outside the open projects"),
            Self::SourceGone => write!(f, "the file no longer exists"),
            Self::OpenInTab => write!(f, "the file is open in a tab"),
            Self::SaveInProgress => write!(f, "a save is being written"),
            Self::ConversionRunning => write!(f, "a conversion is writing this file"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for TrashRefusal {}

impl From<RenameRefusal> for TrashRefusal {
    fn from(refusal: RenameRefusal) -> Self {
        match refusal {
            RenameRefusal::NotADesignFile => Self::NotADesignFile,
            RenameRefusal::OutsideProjects => Self::OutsideProjects,
            RenameRefusal::SourceGone => Self::SourceGone,
            other => Self::Io(other.to_string()),
        }
    }
}

impl DesktopApp {
    /// Execute the move the dock confirmed, if any. Returns whether a request
    /// was taken (the tree needs a republish either way).
    pub(crate) fn pump_project_trash(&mut self) -> bool {
        let Some(path) = self
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .trash_commit
            .take()
        else {
            return false;
        };
        let outcome = self.trash_design_file(Path::new(&path));
        if let Err(refusal) = outcome {
            eprintln!("[projects] trash {path}: {refusal}");
            let key = if refusal == TrashRefusal::OpenInTab {
                TRASH_OPEN_TAB_KEY
            } else {
                TRASH_FAILED_KEY
            };
            let now_ms = self.host.now_ms();
            self.host.editor_state_mut().editor_ui.show_toast(
                key,
                vec![("reason".to_string(), refusal.to_string())],
                EditorToastLevel::Warn,
                now_ms,
            );
        }
        self.projects_host.published_key = None;
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
        true
    }

    /// Move the design at `source` to the Trash. Re-validates everything; the
    /// widget's checks are not trusted.
    pub(crate) fn trash_design_file(&mut self, source: &Path) -> Result<(), TrashRefusal> {
        let backend = Arc::clone(&self.projects_host.trash);
        if !backend.supported() {
            return Err(TrashRefusal::Unsupported);
        }
        let roots: Vec<_> = self.shown_project_roots().into_iter().collect();
        locate_design_file(&roots, source)?;
        if self.projects_host.figs.is_writing(source) {
            return Err(TrashRefusal::ConversionRunning);
        }
        self.sync_active_slot();
        if !self.documents.indices_of_path(source).is_empty() {
            return Err(TrashRefusal::OpenInTab);
        }
        // A queued or running save may be writing this very path (a Save As
        // onto it); it would bring the file back after the move.
        if self.save_session.is_active() {
            return Err(TrashRefusal::SaveInProgress);
        }
        backend
            .trash(source)
            .map_err(|error| TrashRefusal::Io(error.to_string()))?;
        self.follow_trashed_file(source);
        Ok(())
    }

    /// The file is in the Trash: forget it everywhere it was remembered.
    fn follow_trashed_file(&mut self, path: &Path) {
        crate::settings_io::forget_recent(&mut self.host, path);
        self.projects_host.roots.invalidate(path);
        self.projects_host.scan.remove_listed(path);
        self.rescan_after_own_operation();
    }
}

#[cfg(test)]
#[path = "projects_host_trash_tests.rs"]
mod tests;
