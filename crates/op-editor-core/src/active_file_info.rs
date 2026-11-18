//! What the TopBar's info bar says about the document on screen.
//!
//! The info bar replaced the document tab strip: the Projects dock lists the
//! files, the bar names the active one. Everything it shows is derived here,
//! from state that already exists, so the widget never reaches into the host:
//! the project and folder come from the dock rows the host mirrors into
//! `EditorUiState.projects_panel`, the save state from the dirty flag and the
//! file name, the branch from the Git panel.
//!
//! This module is pure data and wasm32-clean. Strings that need a locale (the
//! untitled placeholder) are passed in by the caller.

use crate::document_tabs::{DocumentId, DocumentTabStatus};
use crate::editor_ui_state::EditorUiState;
use crate::page_switch::PageSummary;
use crate::state::EditorState;
use crate::EmbedHost;

/// Where the active document stands against its file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaveState {
    /// Bound to a file and unchanged since it was opened or last saved.
    #[default]
    Saved,
    /// Bound to a file and changed since.
    Edited,
    /// Never saved: there is no file yet.
    Untitled,
    /// Bound to a file that is no longer on disk (deleted or moved outside
    /// the app). The document is kept; Save As keeps the work.
    Missing,
}

/// An agent run in a document other than the one on screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunningElsewhere {
    /// How many other documents have a run live.
    pub count: usize,
    /// The first of them in open order: its id (what a click switches to) and
    /// its file name (what the bar says).
    pub first: Option<(DocumentId, String)>,
}

/// The active document as the info bar shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActiveFileInfo {
    /// The project the file belongs to; empty when it has none.
    pub project: String,
    /// The folder inside the project, `/`-separated; empty for the root.
    pub folder: String,
    /// File name, or the untitled placeholder.
    pub file_name: String,
    /// Where the file lives, when the dock lists it: what the breadcrumb asks
    /// the dock to reveal. `None` for an untitled document, a file no project
    /// owns, and hosts without the dock: the breadcrumb is plain text then.
    pub file_path: Option<String>,
    pub save: SaveState,
    /// The branch of the repository the file sits in, when known.
    pub git_branch: Option<String>,
    /// How many OTHER open documents have unsaved changes: the one on screen
    /// already says so itself.
    pub other_unsaved: usize,
    /// Runs live in other documents (a run in the one on screen shows in the
    /// chat and the save marker instead).
    pub running_elsewhere: RunningElsewhere,
    /// The pages of the document, when the page chip is offered: more than one
    /// page, and a host with the dock. The pages live in the editor state, not
    /// in the UI state, so only [`Self::with_pages_of`] fills this.
    pub pages: Option<PageSummary>,
}

impl ActiveFileInfo {
    /// Derive the info from the UI state. `untitled` is the localized
    /// placeholder for a document with no file name.
    ///
    /// The project, folder and missing mark come from the dock row that is
    /// flagged `active`; a host without the dock (the web) has no such row and
    /// shows the name alone.
    pub fn from_ui(ui: &EditorUiState, untitled: &str) -> Self {
        let dock_row = dock_row_of_active_file(ui);
        let untitled_document = ui.file_name_display.is_none();
        let save = if untitled_document {
            SaveState::Untitled
        } else if dock_row.is_some_and(|(_, file)| file.missing) {
            SaveState::Missing
        } else if ui.document_dirty {
            SaveState::Edited
        } else {
            SaveState::Saved
        };
        Self {
            project: dock_row
                .map(|(project, _)| project.name.clone())
                .unwrap_or_default(),
            folder: dock_row
                .map(|(_, file)| file.folder.clone())
                .unwrap_or_default(),
            file_name: ui
                .file_name_display
                .clone()
                .unwrap_or_else(|| untitled.to_string()),
            file_path: dock_row.map(|(_, file)| file.path.clone()),
            save,
            git_branch: ui.git_panel.branch.clone(),
            other_unsaved: other_tabs(ui).filter(|tab| tab.dirty).count(),
            running_elsewhere: running_elsewhere(ui),
            pages: None,
        }
    }

    /// Add the page list of `state` when the page chip is offered. The chip
    /// needs a second page to switch to, so a single-page document (and the
    /// web, which has no dock and no page chip in v1) gets none.
    pub fn with_pages_of(mut self, state: &EditorState) -> Self {
        self.pages = info_bar_has_dock(&state.editor_ui)
            .then(|| state.page_summary())
            .filter(|summary| summary.names.len() >= 2);
        self
    }

    /// Whether the document has unsaved changes, in any state that has a
    /// marker for it: the dot an elided bar keeps.
    pub fn shows_unsaved_mark(&self) -> bool {
        matches!(
            self.save,
            SaveState::Edited | SaveState::Untitled | SaveState::Missing
        )
    }
}

/// The open documents other than the one on screen. Empty where the bar has no
/// tabs to look at (the web, touch chrome, a VS Code embed).
fn other_tabs(ui: &EditorUiState) -> impl Iterator<Item = &crate::document_tabs::DocumentTabView> {
    let shown = info_bar_has_dock(ui);
    ui.document_tabs
        .iter()
        .filter(move |tab| shown && !tab.active)
}

fn running_elsewhere(ui: &EditorUiState) -> RunningElsewhere {
    let mut running = other_tabs(ui).filter(|tab| tab.status == DocumentTabStatus::Running);
    let first = running.next().map(|tab| (tab.id, tab.label.clone()));
    RunningElsewhere {
        count: usize::from(first.is_some()) + running.count(),
        first,
    }
}

/// The dock row of the document on screen, with its project. `None` when the
/// host lists no such row (web, an untitled document, a file no project owns).
fn dock_row_of_active_file(
    ui: &EditorUiState,
) -> Option<(
    &crate::project_tree::ProjectView,
    &crate::project_tree::ProjectFileView,
)> {
    if !info_bar_has_dock(ui) {
        return None;
    }
    ui.projects_panel.projects.iter().find_map(|project| {
        project
            .files
            .iter()
            .find(|file| file.active)
            .map(|file| (project, file))
    })
}

/// The dock-backed parts of the bar (project, folder, missing mark) exist only
/// where the dock does: a host that holds several documents, not touch chrome,
/// and not a VS Code embed.
fn info_bar_has_dock(ui: &EditorUiState) -> bool {
    ui.projects_dock_available() && ui.embed != EmbedHost::VsCode
}

#[cfg(test)]
#[path = "active_file_info_tests.rs"]
mod tests;
