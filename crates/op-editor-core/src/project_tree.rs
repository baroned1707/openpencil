//! Project -> files tree — the plain-data contract between the host's
//! project scanner / document registry and the left rail's Projects tab.
//!
//! One project can hold several design files, and the owner reviews work
//! across projects. The host mirrors what it knows into [`ProjectView`]
//! rows on `EditorUiState.projects_panel`; the tab only reads those rows and
//! records what the user asked for in `ProjectsPanelState.pending_request`,
//! which the host drains. Neither side reaches into the other's types, and
//! everything here is wasm32-clean.

use crate::document_tabs::{DocumentId, DocumentTabStatus};

/// One design file inside a project, as the Projects tab draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileView {
    /// Where the file lives; what [`ProjectRequest::OpenFile`] carries.
    pub path: String,
    /// File name shown in the row.
    pub label: String,
    /// The open document showing this file, when it is open in a tab.
    pub open_id: Option<DocumentId>,
    pub dirty: bool,
    /// What an agent is doing in the file (only meaningful while open).
    pub status: DocumentTabStatus,
    /// Whether this is the document the editor is showing right now.
    pub active: bool,
}

/// One project: a root folder and the design files found under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectView {
    /// Project root folder; the stable key of the project.
    pub root: String,
    pub name: String,
    pub files: Vec<ProjectFileView>,
    pub expanded: bool,
}

impl ProjectView {
    /// Files an agent has finished and the owner has yet to look at.
    pub fn needs_review_count(&self) -> usize {
        self.count_status(DocumentTabStatus::NeedsReview)
    }

    /// Files an agent is working in right now.
    pub fn running_count(&self) -> usize {
        self.count_status(DocumentTabStatus::Running)
    }

    /// Whether any file has unsaved changes.
    pub fn any_dirty(&self) -> bool {
        self.files.iter().any(|file| file.dirty)
    }

    fn count_status(&self, status: DocumentTabStatus) -> usize {
        self.files
            .iter()
            .filter(|file| file.status == status)
            .count()
    }
}

/// A user request raised by the Projects tab, executed by the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectRequest {
    /// Open the file at this path (it is not open yet).
    OpenFile(String),
    /// Show the already-open document.
    SwitchTo(DocumentId),
    /// Expand or collapse the project with this root. The host owns the
    /// flag: it flips it and mirrors the rows back.
    ToggleProject(String),
    /// Create a new design inside the project with this root.
    NewDesignIn(String),
}

/// What the cursor is over in the Projects tab. Indices point into
/// `ProjectsPanelState.projects`; a stale one simply matches no row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRowTarget {
    /// A project header (toggles it).
    Header(usize),
    /// The header's `+` button (new design in that project).
    Plus(usize),
    /// A file row: `(project index, file index)`.
    File(usize, usize),
    /// The "New design" row an empty project shows in place of files.
    NewDesignRow(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(status: DocumentTabStatus, dirty: bool) -> ProjectFileView {
        ProjectFileView {
            path: "a.op".to_string(),
            label: "a.op".to_string(),
            open_id: None,
            dirty,
            status,
            active: false,
        }
    }

    fn project(files: Vec<ProjectFileView>) -> ProjectView {
        ProjectView {
            root: "/p".to_string(),
            name: "p".to_string(),
            files,
            expanded: true,
        }
    }

    #[test]
    fn needs_review_aggregate_counts_files() {
        let view = project(vec![
            file(DocumentTabStatus::NeedsReview, false),
            file(DocumentTabStatus::Running, false),
            file(DocumentTabStatus::NeedsReview, true),
            file(DocumentTabStatus::Idle, false),
        ]);
        assert_eq!(view.needs_review_count(), 2);
        assert_eq!(view.running_count(), 1);
    }

    #[test]
    fn any_dirty_is_true_when_one_file_is_unsaved() {
        assert!(!project(vec![file(DocumentTabStatus::Idle, false)]).any_dirty());
        assert!(project(vec![
            file(DocumentTabStatus::Idle, false),
            file(DocumentTabStatus::Done, true),
        ])
        .any_dirty());
        assert!(!project(Vec::new()).any_dirty());
    }
}
