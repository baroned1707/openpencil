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
    /// Not persisted across launches: a fresh window always opens it. Only
    /// read while there is a project; see [`ProjectsPanelState::is_open`].
    pub dock_open: bool,
    /// The same choice while there is NO project: the empty dock starts as a
    /// slim strip so it does not claim a column for nothing, and the user can
    /// unfold it to read the hint.
    pub empty_open: bool,
    /// The cursor is over the dock's collapse / expand chevron.
    pub toggle_hover: bool,
    /// The cursor is over the dock's "Open project" button.
    pub open_project_hover: bool,
}

impl Default for ProjectsPanelState {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            pending_request: None,
            hover: None,
            scroll: ScrollState::default(),
            dock_open: true,
            empty_open: false,
            toggle_hover: false,
            open_project_hover: false,
        }
    }
}

impl ProjectsPanelState {
    /// Whether the dock shows its list. The one answer every consumer (canvas
    /// geometry, hit-testing, paint) reads: an empty dock is open only when the
    /// user unfolded it, a dock with projects is open unless they folded it.
    pub fn is_open(&self) -> bool {
        if self.projects.is_empty() {
            self.empty_open
        } else {
            self.dock_open
        }
    }

    /// Fold or unfold the dock (the choice that applies right now).
    pub fn toggle_open(&mut self) {
        if self.projects.is_empty() {
            self.empty_open = !self.empty_open;
        } else {
            self.dock_open = !self.dock_open;
        }
    }
}

impl EditorUiState {
    /// Whether the window offers the Projects dock at its right edge.
    ///
    /// Gated exactly like the open-document tab strip: only on a host that
    /// can hold several documents, and never on touch chrome. With no project
    /// the dock is still there as a slim strip, because it is where a new user
    /// finds "Open project".
    pub fn projects_dock_available(&self) -> bool {
        self.document_tabs_supported && !self.touch_chrome()
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
            ..ProjectView::default()
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
    fn projects_dock_is_available_without_projects() {
        let mut ui = EditorUiState::default();
        ui.document_tabs_supported = true;
        assert!(ui.projects_dock_available());
    }

    #[test]
    fn an_empty_dock_starts_folded_and_a_dock_with_projects_starts_open() {
        let mut ui = EditorUiState::default();
        assert!(!ui.projects_panel.is_open(), "empty: slim strip");
        ui.projects_panel.projects = vec![project()];
        assert!(ui.projects_panel.is_open(), "first project opens the dock");
        ui.projects_panel.toggle_open();
        assert!(!ui.projects_panel.is_open());
        ui.projects_panel.projects.clear();
        assert!(
            !ui.projects_panel.is_open(),
            "the empty dock keeps its own choice"
        );
        ui.projects_panel.toggle_open();
        assert!(ui.projects_panel.is_open());
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
