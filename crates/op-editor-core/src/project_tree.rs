//! Project -> files tree — the plain-data contract between the host's
//! project scanner / document registry and the Projects dock.
//!
//! One project can hold several design files, and the owner reviews work
//! across projects. The host mirrors what it knows into [`ProjectView`]
//! rows on `EditorUiState.projects_panel`; the tab only reads those rows and
//! records what the user asked for in `ProjectsPanelState.pending_request`,
//! which the host drains. Neither side reaches into the other's types, and
//! everything here is wasm32-clean.

use crate::document_tabs::{DocumentId, DocumentTabStatus};

/// Where a Figma `.fig` file stands on its way to an `.op` file next to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FigStatus {
    /// No `<stem>.op` beside it yet; clicking the row converts it.
    NotConverted,
    /// `<stem>.op` exists beside it; clicking the row opens that file.
    Converted,
    /// A conversion is queued or running.
    Converting,
    /// The last conversion failed; the short reason is shown as a tooltip and
    /// clicking the row tries again.
    Failed(String),
}

/// What kind of file a project row stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProjectFileKind {
    /// An `.op` design (the default).
    #[default]
    Design,
    /// A Figma `.fig` export, listed so it can be converted.
    Figma(FigStatus),
}

impl ProjectFileKind {
    /// Whether this row is a `.fig` that has not been converted.
    pub fn is_not_converted(&self) -> bool {
        matches!(self, Self::Figma(FigStatus::NotConverted))
    }
}

/// One design file inside a project, as the Projects dock draws it.
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
    /// The directory of the file inside the project, relative to the project
    /// root with `/` separators; empty for a file directly in the root. The
    /// dock groups rows under this.
    pub folder: String,
    /// `.op` design or `.fig` export with its conversion status.
    pub kind: ProjectFileKind,
}

/// How far the last disk scan of a project got; the dock shows a status line
/// for everything but [`ProjectScanState::Idle`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectScanState {
    /// The listing is complete and no scan is running.
    #[default]
    Idle,
    /// The first scan (or a requested refresh) has not finished yet.
    Scanning,
    /// The scan ran out of time; the listing is what was found until then.
    Slow,
    /// More files than fit; this many were left out of the listing.
    Truncated(usize),
    /// The folder could not be read.
    Unreadable,
}

/// One project: a root folder and the design files found under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectView {
    /// Project root folder; the stable key of the project.
    pub root: String,
    pub name: String,
    /// Grouped by folder: the files of one folder are contiguous, the files
    /// directly in the root first.
    pub files: Vec<ProjectFileView>,
    pub expanded: bool,
    /// The user opened this folder explicitly, so the dock offers to close it.
    pub closable: bool,
    pub scan: ProjectScanState,
    /// Relative folders (`ProjectFileView::folder`) the user folded; their
    /// files stay in `files` but the dock hides them.
    pub collapsed_folders: Vec<String>,
}

impl Default for ProjectView {
    /// An empty, expanded, inferred project; callers fill in what they know.
    fn default() -> Self {
        Self {
            root: String::new(),
            name: String::new(),
            files: Vec::new(),
            expanded: true,
            closable: false,
            scan: ProjectScanState::Idle,
            collapsed_folders: Vec::new(),
        }
    }
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

    /// Whether the dock hides the files of `folder`.
    pub fn is_folder_collapsed(&self, folder: &str) -> bool {
        self.collapsed_folders.iter().any(|name| name == folder)
    }

    /// `.fig` files not converted yet: those under `folder` and its subfolders
    /// (a relative folder, `""` for the root), or the whole project for `None`.
    pub fn not_converted_count(&self, folder: Option<&str>) -> usize {
        self.files
            .iter()
            .filter(|file| file.kind.is_not_converted() && in_folder_tree(&file.folder, folder))
            .count()
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

/// A user request raised by the Projects dock, executed by the host.
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
    /// Pick a folder and open it as a project.
    OpenProjectPicker,
    /// Stop treating the explicitly opened project with this root as one.
    CloseProject(String),
    /// Scan the project with this root again.
    RefreshProject(String),
    /// Fold or unfold a folder: `(project root, relative folder)`.
    ToggleFolder(String, String),
    /// A click on the `.fig` file at this path: convert it (and open the result),
    /// open the `.op` beside it when that exists, or retry a failed conversion.
    /// The host decides which, from what is on disk.
    ConvertFig(String),
    /// Convert every not-yet-converted `.fig` under a folder: `(project root,
    /// relative folder)`, or the whole project when the folder is `None`. Never
    /// opens a tab.
    ConvertAllFigs(String, Option<String>),
}

/// What the cursor is over in the Projects dock. Indices point into
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
    /// A folder sub-header: `(project index, index of the folder's first file)`.
    Folder(usize, usize),
    /// The header's refresh button (explicit projects, while idle).
    Refresh(usize),
    /// The header's close button (explicit projects).
    Close(usize),
    /// The empty dock's "Open project" button.
    OpenProject,
    /// A "Convert all" row: `(project index, index of the first file of the
    /// folder it belongs to)`, or `None` for the whole project.
    ConvertAll(usize, Option<usize>),
}

/// Whether `file_folder` is `folder` or below it; `None` matches everything.
pub fn in_folder_tree(file_folder: &str, folder: Option<&str>) -> bool {
    match folder {
        None => true,
        Some("") => true,
        Some(folder) => {
            file_folder == folder
                || file_folder
                    .strip_prefix(folder)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    }
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
            folder: String::new(),
            kind: ProjectFileKind::Design,
        }
    }

    fn project(files: Vec<ProjectFileView>) -> ProjectView {
        ProjectView {
            root: "/p".to_string(),
            name: "p".to_string(),
            files,
            ..ProjectView::default()
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
    fn not_converted_count_scopes_to_a_folder_tree() {
        let fig = |folder: &str, status: FigStatus| ProjectFileView {
            folder: folder.to_string(),
            kind: ProjectFileKind::Figma(status),
            ..file(DocumentTabStatus::Idle, false)
        };
        let view = project(vec![
            fig("", FigStatus::NotConverted),
            fig("a", FigStatus::NotConverted),
            fig("a/b", FigStatus::NotConverted),
            fig("ab", FigStatus::NotConverted),
            fig("a", FigStatus::Converted),
            fig("a", FigStatus::Failed("x".to_string())),
            file(DocumentTabStatus::Idle, false),
        ]);
        assert_eq!(view.not_converted_count(None), 4);
        assert_eq!(view.not_converted_count(Some("")), 4);
        assert_eq!(view.not_converted_count(Some("a")), 2, "a and a/b, not ab");
        assert_eq!(view.not_converted_count(Some("a/b")), 1);
        assert_eq!(view.not_converted_count(Some("zzz")), 0);
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
