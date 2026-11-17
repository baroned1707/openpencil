//! Opening, closing and refreshing the folders the user picked as projects.
//!
//! An opened folder is a project root whether or not it carries a marker file.
//! The set lives in the document registry (`DocumentRegistry::explicit_roots`),
//! because that is what names a tab's project; this module is the only writer.
//! Opening one refuses the places that would list a whole machine
//! ([`ProjectOpenError::Unsafe`]), registers the canonical folder, queues its
//! recursive walk and saves the list, so the next launch restores it BEFORE the
//! documents (their tabs then group under it). Closing one forgets the folder
//! but never closes a tab.

use std::fmt;
use std::path::{Path, PathBuf};

use op_editor_core::editor_toast::EditorToastLevel;
use op_host_services::project_root::{self, UnsafeRoot};

use super::{FolderPicker, ProjectsHostState};
use crate::DesktopApp;

/// Toast shown when a folder is too broad to be a project.
const UNSAFE_ROOT_KEY: &str = "project.unsafeRoot";
/// Toast shown when a folder cannot be opened at all.
const UNREADABLE_KEY: &str = "project.unreadable";

/// Why a folder was not opened as a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectOpenError {
    /// The path is not a folder, or it cannot be read.
    Unreadable(std::io::ErrorKind),
    /// The folder is the filesystem root, the home directory (or a folder
    /// holding it) or a system directory.
    Unsafe(UnsafeRoot),
}

impl fmt::Display for ProjectOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(kind) => write!(f, "the folder cannot be read: {kind}"),
            Self::Unsafe(reason) => write!(f, "refusing to open {reason} as a project"),
        }
    }
}

impl std::error::Error for ProjectOpenError {}

/// The real folder dialog.
pub(super) fn native_folder_picker() -> FolderPicker {
    Box::new(|title| rfd::FileDialog::new().set_title(title).pick_folder())
}

impl ProjectsHostState {
    /// A state that restores `saved` (the folders open at the last quit) once
    /// the window is up.
    pub(crate) fn restoring(saved: Vec<String>) -> Self {
        Self {
            pending_restore: saved,
            ..Self::default()
        }
    }

    /// Replace the folder dialog (tests outside this module inject one).
    #[cfg(test)]
    pub(crate) fn set_folder_picker(&mut self, picker: FolderPicker) {
        self.picker = picker;
    }

    /// `path` as an opened-project root: canonical, a readable folder, and not a
    /// place that must never be a project.
    fn admit(&self, path: &Path) -> Result<PathBuf, ProjectOpenError> {
        let canonical = std::fs::canonicalize(path)
            .map_err(|error| ProjectOpenError::Unreadable(error.kind()))?;
        let meta = std::fs::metadata(&canonical)
            .map_err(|error| ProjectOpenError::Unreadable(error.kind()))?;
        if !meta.is_dir() {
            return Err(ProjectOpenError::Unreadable(
                std::io::ErrorKind::NotADirectory,
            ));
        }
        let home = match &self.home_override {
            Some(home) => home.clone(),
            None => project_root::canonical_home(),
        };
        project_root::check_project_root(&canonical, home.as_deref(), &self.system_dirs)
            .map_err(ProjectOpenError::Unsafe)?;
        Ok(canonical)
    }
}

impl DesktopApp {
    /// File ▸ Open Project: ask for a folder and open it. A cancel changes
    /// nothing.
    pub(crate) fn open_project_via_picker(&mut self) -> bool {
        self.host.commit_variable_row_focus_if_any_pub();
        let locale = self.host.editor_state().editor_ui.locale;
        let title = op_i18n::translate(locale, "project.open").trim_end_matches(['.', '…']);
        match (self.projects_host.picker)(title) {
            Some(folder) => self.open_project(&folder),
            None => false,
        }
    }

    /// Open `folder` as a project, or say (toast) why it cannot be one. Opening
    /// a folder that already is one scans it again and brings it into view.
    pub(crate) fn open_project(&mut self, folder: &Path) -> bool {
        let root = match self.projects_host.admit(folder) {
            Ok(root) => root,
            Err(error) => {
                eprintln!("[projects] open {}: {error}", folder.display());
                let key = match error {
                    ProjectOpenError::Unsafe(_) => UNSAFE_ROOT_KEY,
                    ProjectOpenError::Unreadable(_) => UNREADABLE_KEY,
                };
                self.toast_project(key, EditorToastLevel::Warn);
                return false;
            }
        };
        self.register_project_root(root.clone());
        let state = &mut self.projects_host;
        state.collapsed.remove(&root);
        // The dock folds out so the user sees what they opened.
        let panel = &mut self.host.editor_state_mut().editor_ui.projects_panel;
        panel.dock_open = true;
        self.sync_open_projects_setting();
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
        true
    }

    /// Reopen the folders saved at the last quit, BEFORE the documents so the
    /// tabs group under them. A folder that is gone, unreadable or no longer
    /// safe is skipped and pruned from the saved list without a word.
    pub(crate) fn restore_open_projects(&mut self) {
        let saved = std::mem::take(&mut self.projects_host.pending_restore);
        for entry in saved {
            if let Ok(root) = self.projects_host.admit(Path::new(&entry)) {
                self.register_project_root(root);
            }
        }
        self.sync_open_projects_setting();
    }

    /// Stop treating `root` as an opened project. Its tabs stay open (they fall
    /// back to the project they would have had without it).
    pub(crate) fn close_project(&mut self, root: &Path) {
        if !self.documents.explicit_roots().remove(root) {
            return;
        }
        let state = &mut self.projects_host;
        state.known.remove(root);
        state.scan.forget_root(root);
        state.awaiting.remove(root);
        state.collapsed.remove(root);
        state
            .collapsed_folders
            .retain(|(project, _)| project != root);
        state.published_key = None;
        self.sync_open_projects_setting();
        self.host.mark_editor_state_dirty();
    }

    /// Scan `root` again: an opened project is walked afresh, an inferred one
    /// has its `designs/` folder re-read.
    pub(crate) fn refresh_project(&mut self, root: &Path) {
        if self.documents.explicit_roots().contains(root) {
            let state = &mut self.projects_host;
            let serial = state.scan.request_tree_scan(root);
            state.awaiting.insert(root.to_path_buf(), serial);
        } else {
            self.projects_host.scan.request_now();
        }
        self.projects_host.published_key = None;
    }

    /// Record `root` as opened and queue its walk. No toast, no dock change.
    fn register_project_root(&mut self, root: PathBuf) {
        self.documents.explicit_roots().insert(root.clone());
        let state = &mut self.projects_host;
        // Projects inferred from documents inside the folder are part of it now.
        let inside: Vec<PathBuf> = state
            .known
            .iter()
            .filter(|known| *known != &root && known.starts_with(&root))
            .cloned()
            .collect();
        for known in inside {
            state.known.remove(&known);
            state.scan.forget_root(&known);
        }
        state.known.insert(root.clone());
        let serial = state.scan.request_tree_scan(&root);
        state.awaiting.insert(root, serial);
        state.published_key = None;
    }

    /// Mirror the opened folders into the settings and save when they changed.
    fn sync_open_projects_setting(&mut self) {
        let roots: Vec<String> = self
            .documents
            .explicit_roots()
            .snapshot()
            .iter()
            .map(|root| root.to_string_lossy().into_owned())
            .collect();
        if self.host.editor_state().editor_ui.open_projects == roots {
            return;
        }
        let before = op_host_services::settings_io::fingerprint(self.host.editor_state());
        self.host.editor_state_mut().editor_ui.open_projects = roots;
        op_host_services::settings_io::save_if_changed(self.host.editor_state(), before);
    }

    /// Give the live MCP server the opened folders as extra allowed roots, only
    /// when they changed since it was last told.
    pub(crate) fn sync_mcp_project_roots(&mut self) {
        let Some(server) = self.mcp_server.as_ref() else {
            return;
        };
        let roots = self.documents.explicit_roots().snapshot();
        if self.projects_host.mcp_roots_sent.as_ref() == Some(&roots) {
            return;
        }
        server.set_extra_roots(roots.clone());
        self.projects_host.mcp_roots_sent = Some(roots);
    }

    fn toast_project(&mut self, key: &'static str, level: EditorToastLevel) {
        let now_ms = self.host.now_ms();
        self.host
            .editor_state_mut()
            .editor_ui
            .show_toast(key, Vec::new(), level, now_ms);
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }
}
