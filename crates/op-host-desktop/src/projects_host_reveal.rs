//! "Show in the dock", on the desktop side: the info bar's breadcrumb asks the
//! dock to reveal the row of the file on screen.
//!
//! Revealing is three things. The host opens the dock when it is folded and
//! unfolds the project and the folders above the file (those folds are host
//! state, see [`op_editor_core::project_reveal`]); the rows are rebuilt with
//! the folds open; and only then does the native host scroll the row into view,
//! because only the rebuilt rows have a layout to scroll. The row of the active
//! file already carries the selected fill, so nothing else is painted. If the
//! dock no longer lists the file there is nothing to reveal and nothing happens.

use std::path::PathBuf;

use op_editor_core::project_reveal::reveal_plan;

use crate::DesktopApp;

impl DesktopApp {
    /// Carry out `ProjectRequest::RevealFile(path)`: open the dock, unfold what
    /// hides the row, and remember to scroll it into view once the rows are
    /// rebuilt. A file no project lists is ignored.
    pub(super) fn reveal_file_in_dock(&mut self, path: &str) {
        let panel = &mut self.host.editor_state_mut().editor_ui.projects_panel;
        let Some(plan) = reveal_plan(&panel.projects, path) else {
            return;
        };
        panel.open_dock();
        let root = PathBuf::from(&plan.project_root);
        let state = &mut self.projects_host;
        if plan.unfold_project {
            state.collapsed.remove(&root);
        }
        for folder in plan.unfold_folders {
            state.collapsed_folders.remove(&(root.clone(), folder));
        }
        state.pending_reveal = Some(path.to_string());
    }

    /// After the rows were rebuilt: scroll the row to reveal into view. `true`
    /// when the dock changed.
    pub(super) fn finish_pending_reveal(&mut self) -> bool {
        let Some(path) = self.projects_host.pending_reveal.take() else {
            return false;
        };
        self.host.reveal_project_file(&path)
    }
}

#[cfg(test)]
#[path = "projects_host_reveal_tests.rs"]
mod tests;
