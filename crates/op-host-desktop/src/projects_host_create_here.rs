//! "New design here" from the Projects dock, on the desktop side: create
//! `Untitled.op` in a folder of a shown project and start naming it.
//!
//! The dock sends `(project root, relative folder)`. The host re-validates the
//! pair against the live state and the disk ([`plan_folder`]): the root is one
//! the dock shows, the folder is a real directory (no link anywhere on the way)
//! that sits inside the root, and the project would actually list a file there.
//! A folder that has vanished fails; it is never recreated. The file is written
//! with the no-replace writer `New design` uses, so a name taken meanwhile moves
//! on to the next one. It is NOT opened in a tab: the listing is patched, the
//! folder unfolded and the inline rename started on the new row, so the owner
//! names it at once and opens it only by clicking it.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use op_editor_core::editor_toast::EditorToastLevel;

use super::new_design::{create_untitled_in, NewDesignError};
use super::scan_tree::SKIPPED_DIRECTORIES;
use crate::DesktopApp;

/// i18n key of the toast raised when the design could not be created (shared
/// with the `+` button).
const NEW_DESIGN_FAILED_KEY: &str = "projects.newDesignFailed";

/// Why no design was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreateHereRefusal {
    /// The root is not a project the dock shows.
    UnknownProject,
    /// The relative folder is empty of meaning (`..`, an absolute path, ...).
    BadFolder,
    /// The folder is hidden, skipped by the scan, or outside what this kind of
    /// project lists: the new file would never show up.
    NotListed,
    /// The folder no longer exists (or is not a folder).
    FolderGone,
    /// A link on the way leads somewhere else, or out of the project.
    OutsideProject,
    /// The disk refused.
    Io(String),
    /// The file could not be written.
    Create(NewDesignError),
}

impl fmt::Display for CreateHereRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProject => write!(f, "the project is not open"),
            Self::BadFolder => write!(f, "that folder cannot be used"),
            Self::NotListed => write!(f, "the project does not list files in that folder"),
            Self::FolderGone => write!(f, "the folder no longer exists"),
            Self::OutsideProject => write!(f, "the folder is outside the project"),
            Self::Io(message) => write!(f, "{message}"),
            Self::Create(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CreateHereRefusal {}

/// The folder `relative` below `root`, after every check. `explicit` says the
/// project is a folder the user opened (every folder in it is walked); an
/// inferred project lists only its `designs/` folder.
pub(crate) fn plan_folder(
    roots: &[PathBuf],
    explicit: bool,
    root: &Path,
    relative: &str,
) -> Result<PathBuf, CreateHereRefusal> {
    if !roots.iter().any(|shown| shown == root) {
        return Err(CreateHereRefusal::UnknownProject);
    }
    let mut parts: Vec<&str> = Vec::new();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => {
                parts.push(part.to_str().ok_or(CreateHereRefusal::BadFolder)?)
            }
            _ => return Err(CreateHereRefusal::BadFolder),
        }
    }
    if parts
        .iter()
        .any(|part| part.starts_with('.') || SKIPPED_DIRECTORIES.contains(part))
        || (!explicit && parts != ["designs"])
    {
        return Err(CreateHereRefusal::NotListed);
    }
    let folder = parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let meta = std::fs::symlink_metadata(&folder).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => CreateHereRefusal::FolderGone,
        _ => CreateHereRefusal::Io(error.to_string()),
    })?;
    if meta.file_type().is_symlink() {
        return Err(CreateHereRefusal::OutsideProject);
    }
    if !meta.is_dir() {
        return Err(CreateHereRefusal::FolderGone);
    }
    // No link anywhere on the way: the real path is the root's real path plus
    // the relative parts, exactly.
    let canonical_root =
        std::fs::canonicalize(root).map_err(|error| CreateHereRefusal::Io(error.to_string()))?;
    let canonical_folder =
        std::fs::canonicalize(&folder).map_err(|error| CreateHereRefusal::Io(error.to_string()))?;
    let expected = parts
        .iter()
        .fold(canonical_root, |path, part| path.join(part));
    if canonical_folder != expected {
        return Err(CreateHereRefusal::OutsideProject);
    }
    Ok(folder)
}

impl DesktopApp {
    /// Create an untitled design in `relative` below `root` and start renaming
    /// it. A refusal or failure shows a toast and leaves nothing behind.
    pub(crate) fn create_design_here(&mut self, root: &Path, relative: &str) {
        match self.try_create_design_here(root, relative) {
            Ok(target) => {
                let state = &mut self.projects_host;
                state.collapsed.remove(root);
                state
                    .collapsed_folders
                    .remove(&(root.to_path_buf(), relative.to_string()));
                state.pending_rename = Some(target);
                self.host
                    .editor_state_mut()
                    .editor_ui
                    .projects_panel
                    .dock_open = true;
            }
            Err(refusal) => {
                eprintln!("[projects] new design in {}: {refusal}", root.display());
                let now_ms = self.host.now_ms();
                self.host.editor_state_mut().editor_ui.show_toast(
                    NEW_DESIGN_FAILED_KEY,
                    vec![("reason".to_string(), refusal.to_string())],
                    EditorToastLevel::Warn,
                    now_ms,
                );
            }
        }
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }

    fn try_create_design_here(
        &mut self,
        root: &Path,
        relative: &str,
    ) -> Result<PathBuf, CreateHereRefusal> {
        let roots: Vec<PathBuf> = self.shown_project_roots().into_iter().collect();
        let explicit = self.documents.explicit_roots().contains(root);
        let folder = plan_folder(&roots, explicit, root, relative)?;
        let starter = op_editor_core::EditorState::starter();
        let target = create_untitled_in(&folder, &|target| {
            op_host_services::doc_io::save_new_to_path(&starter, target)
        })
        .map_err(CreateHereRefusal::Create)?;
        let state = &mut self.projects_host;
        state.roots.invalidate(&target);
        state.scan.insert_listed(root, &target);
        self.rescan_after_own_operation();
        Ok(target)
    }
}

#[cfg(test)]
#[path = "projects_host_create_here_tests.rs"]
mod tests;
