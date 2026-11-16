//! State behind the Projects dock.
//!
//! The host mirrors its project scan and document registry into
//! [`ProjectsPanelState::projects`]; the dock reads those rows and writes
//! [`ProjectsPanelState::pending_request`], which the host drains each
//! frame. Hover and scroll are transient pointer bookkeeping and live here
//! too, so the whole dock is one substruct on `EditorUiState`.
//!
//! All of it is app-level: the rows describe every project on disk, not one
//! document, so none of it travels with a document scope.

use jian_core::scroll::ScrollState;

use super::EditorUiState;
use crate::project_tree::{ProjectRequest, ProjectRowTarget, ProjectView};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectsPanelState {
    /// Projects and their files, filled by the host.
    pub projects: Vec<ProjectView>,
    /// What the user asked for; drained by the host. Never persisted.
    pub pending_request: Option<ProjectRequest>,
    /// What the cursor is over — drives the hover wash and the `+` button.
    pub hover: Option<ProjectRowTarget>,
    /// Vertical scroll of the project list.
    pub scroll: ScrollState,
    /// Whether the dock shows its list (`true`) or only its slim strip.
    /// Not persisted across launches: a fresh window always opens it.
    pub dock_open: bool,
    /// The cursor is over the dock's collapse / expand chevron.
    pub toggle_hover: bool,
}

impl Default for ProjectsPanelState {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            pending_request: None,
            hover: None,
            scroll: ScrollState::default(),
            dock_open: true,
            toggle_hover: false,
        }
    }
}

impl EditorUiState {
    /// Whether the window offers the Projects dock at its right edge.
    ///
    /// Gated exactly like the open-document tab strip: only on a host that
    /// can hold several documents, never on touch chrome, and only once the
    /// host has reported at least one project — a dock with nothing to list
    /// would be an empty column the user could do nothing with.
    pub fn projects_dock_available(&self) -> bool {
        self.document_tabs_supported
            && !self.touch_chrome()
            && !self.projects_panel.projects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tree::ProjectView;

    fn project() -> ProjectView {
        ProjectView {
            root: "/p".to_string(),
            name: "p".to_string(),
            files: Vec::new(),
            expanded: true,
        }
    }

    #[test]
    fn projects_dock_hidden_when_unsupported() {
        let mut ui = EditorUiState::default();
        ui.projects_panel.projects = vec![project()];
        ui.document_tabs_supported = false;
        assert!(!ui.projects_dock_available());
        ui.document_tabs_supported = true;
        assert!(ui.projects_dock_available());
    }

    #[test]
    fn projects_dock_hidden_without_projects() {
        let mut ui = EditorUiState::default();
        ui.document_tabs_supported = true;
        assert!(!ui.projects_dock_available());
    }

    #[test]
    fn projects_dock_hidden_on_touch_chrome() {
        let mut ui = EditorUiState::default();
        ui.document_tabs_supported = true;
        ui.projects_panel.projects = vec![project()];
        ui.touch = true;
        assert!(!ui.projects_dock_available());
    }
}
