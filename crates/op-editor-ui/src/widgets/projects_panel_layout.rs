//! Geometry of the Projects dock's list: where every row sits for a given scroll
//! offset, and what a point lands on.
//!
//! One pure function of the project rows, the list body rect and the scroll
//! offset. Paint, hit-test and scroll all read the same [`ProjectsPanelLayout`],
//! so a painted row and its click target can never drift apart.

use op_editor_core::{ProjectRowTarget, ProjectScanState, ProjectView};

use crate::{Point2D, Rect};

/// Height of a project header row.
pub const HEADER_HEIGHT: f32 = 32.0;
/// Height of a file row (and of the empty project's "New design" row).
pub const FILE_HEIGHT: f32 = 28.0;
/// Height of a folder sub-header.
pub const FOLDER_HEIGHT: f32 = 24.0;
/// Height of a status line (scanning, truncated, nothing found).
pub const STATUS_HEIGHT: f32 = 24.0;
/// Side of the header's square `+` button (and of its refresh / close twins).
pub const PLUS_SIZE: f32 = 22.0;
/// Space between two header buttons.
const BUTTON_GAP: f32 = 2.0;
/// The empty list's "Open project" button: its top below the list's top, and
/// its height.
const EMPTY_BUTTON_TOP: f32 = 84.0;
const EMPTY_BUTTON_HEIGHT: f32 = 28.0;
const EMPTY_BUTTON_MARGIN_X: f32 = 16.0;
/// Space above the first row and below the last.
const LIST_PAD_TOP: f32 = 4.0;
const LIST_PAD_BOTTOM: f32 = 12.0;
/// Air above every project header but the first, so each project reads as
/// one group instead of a flat list of rows.
const PROJECT_GAP: f32 = 6.0;
/// Rows are washed this far in from the dock's edges, so a selected row
/// reads as a block resting on the dock rather than a stripe through it.
pub const ROW_INSET_X: f32 = 8.0;
/// Right padding inside a row for its trailing content.
pub const ROW_PAD_RIGHT: f32 = 8.0;

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Project header, by project index.
    Header(usize),
    /// Folder sub-header, by `(project index, index of the folder's first
    /// file)`.
    Folder(usize, usize),
    /// File row, by `(project index, file index)`.
    File(usize, usize),
    /// The "New design" row an empty, expanded project shows.
    NewDesign(usize),
    /// A scan status line (scanning, slow, unreadable, `N more not shown`),
    /// by project index; what it says is the project's `scan`.
    Status(usize),
    /// "No designs here yet", the line above an empty project's New design row.
    NoDesigns(usize),
}

/// One laid-out row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectRow {
    pub kind: RowKind,
    /// On-screen rect after scrolling (may lie outside the viewport).
    pub rect: Rect,
    /// A header's `+` button; `None` on every other row.
    pub plus: Option<Rect>,
    /// A header's refresh button: explicit projects, only while idle.
    pub refresh: Option<Rect>,
    /// A header's close button: explicit projects only.
    pub close: Option<Rect>,
}

/// Where the Projects dock's rows sit.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectsPanelLayout {
    /// The viewport: the dock body below its header.
    pub panel: Rect,
    pub rows: Vec<ProjectRow>,
    /// Total height of all rows plus padding.
    pub content_height: f32,
    /// The scroll offset the rows were placed with, already clamped.
    pub offset: f32,
    /// True when there are no projects to list at all.
    pub empty: bool,
    /// The empty list's "Open project" button; `None` when there are projects.
    pub empty_button: Option<Rect>,
}

impl ProjectsPanelLayout {
    /// Lay `projects` out inside `panel` scrolled by `scroll_offset`.
    ///
    /// A collapsed project contributes only its header. An expanded one shows,
    /// in order: a status line while its scan is running, slow or failed; its
    /// files grouped under a sub-header per folder (the files of a folded
    /// folder are skipped); a "N more not shown" line when the listing was cut.
    /// With no files it shows "nothing found" (once the scan is done) and a
    /// "New design" row instead. A stale offset (the list shrank) is clamped,
    /// so the rows never float above the viewport.
    pub fn new(projects: &[ProjectView], panel: Rect, scroll_offset: f32) -> Self {
        // `(kind, height, space above)` in paint order.
        let mut specs: Vec<(RowKind, f32, f32)> = Vec::new();
        for (index, project) in projects.iter().enumerate() {
            let gap = if index == 0 { 0.0 } else { PROJECT_GAP };
            specs.push((RowKind::Header(index), HEADER_HEIGHT, gap));
            if !project.expanded {
                continue;
            }
            if matches!(
                project.scan,
                ProjectScanState::Scanning | ProjectScanState::Slow | ProjectScanState::Unreadable
            ) {
                specs.push((RowKind::Status(index), STATUS_HEIGHT, 0.0));
            }
            if project.files.is_empty() {
                if matches!(
                    project.scan,
                    ProjectScanState::Idle | ProjectScanState::Truncated(_)
                ) {
                    specs.push((RowKind::NoDesigns(index), STATUS_HEIGHT, 0.0));
                }
                specs.push((RowKind::NewDesign(index), FILE_HEIGHT, 0.0));
            }
            let mut current_folder: Option<&str> = None;
            let mut folded = false;
            for (file_index, file) in project.files.iter().enumerate() {
                if current_folder != Some(file.folder.as_str()) {
                    current_folder = Some(file.folder.as_str());
                    folded = !file.folder.is_empty() && project.is_folder_collapsed(&file.folder);
                    if !file.folder.is_empty() {
                        specs.push((RowKind::Folder(index, file_index), FOLDER_HEIGHT, 0.0));
                    }
                }
                if !folded {
                    specs.push((RowKind::File(index, file_index), FILE_HEIGHT, 0.0));
                }
            }
            if matches!(project.scan, ProjectScanState::Truncated(_)) {
                specs.push((RowKind::Status(index), STATUS_HEIGHT, 0.0));
            }
        }
        let rows_height: f32 = specs.iter().map(|(_, height, gap)| height + gap).sum();
        let content_height = if specs.is_empty() {
            0.0
        } else {
            LIST_PAD_TOP + rows_height + LIST_PAD_BOTTOM
        };
        let max = (content_height - panel.size.y).max(0.0);
        let offset = scroll_offset.clamp(0.0, max);

        let x = panel.origin.x + ROW_INSET_X;
        let width = (panel.size.x - ROW_INSET_X * 2.0).max(0.0);
        let mut y = panel.origin.y + LIST_PAD_TOP - offset;
        let rows = specs
            .into_iter()
            .map(|(kind, height, gap)| {
                y += gap;
                let rect = Rect::xywh(x, y, width, height);
                y += height;
                match kind {
                    RowKind::Header(project) => header_row(project, rect, projects.get(project)),
                    _ => ProjectRow {
                        kind,
                        rect,
                        plus: None,
                        refresh: None,
                        close: None,
                    },
                }
            })
            .collect();
        let empty = projects.is_empty();
        Self {
            panel,
            rows,
            content_height,
            offset,
            empty,
            empty_button: empty.then(|| {
                Rect::xywh(
                    panel.origin.x + EMPTY_BUTTON_MARGIN_X,
                    panel.origin.y + EMPTY_BUTTON_TOP,
                    (panel.size.x - EMPTY_BUTTON_MARGIN_X * 2.0).max(0.0),
                    EMPTY_BUTTON_HEIGHT,
                )
            }),
        }
    }

    /// How far the list can scroll.
    pub fn max_scroll(&self) -> f32 {
        (self.content_height - self.panel.size.y).max(0.0)
    }

    /// Whether `point` is on the list body (the list's whole claim).
    pub fn contains_point(&self, point: Point2D) -> bool {
        self.panel.contains(point)
    }

    /// What `point` lands on, or `None` for the gaps and anywhere off the
    /// viewport. A header's buttons win over the header they sit on.
    ///
    /// The buttons are hit-testable wherever they would be painted, hovered
    /// or not: a pointer always moves onto a header before it presses it, so
    /// a button is already showing by the time it can be clicked.
    pub fn hit(&self, point: Point2D) -> Option<ProjectRowTarget> {
        if !self.contains_point(point) {
            return None;
        }
        if self
            .empty_button
            .is_some_and(|button| button.contains(point))
        {
            return Some(ProjectRowTarget::OpenProject);
        }
        self.rows.iter().find_map(|row| {
            if !row.rect.contains(point) {
                return None;
            }
            Some(match row.kind {
                RowKind::Header(project) => {
                    let on = |button: Option<Rect>| button.is_some_and(|rect| rect.contains(point));
                    if on(row.plus) {
                        ProjectRowTarget::Plus(project)
                    } else if on(row.close) {
                        ProjectRowTarget::Close(project)
                    } else if on(row.refresh) {
                        ProjectRowTarget::Refresh(project)
                    } else {
                        ProjectRowTarget::Header(project)
                    }
                }
                RowKind::Folder(project, file) => ProjectRowTarget::Folder(project, file),
                RowKind::File(project, file) => ProjectRowTarget::File(project, file),
                RowKind::NewDesign(project) => ProjectRowTarget::NewDesignRow(project),
                // Status lines are text, not targets.
                RowKind::Status(_) | RowKind::NoDesigns(_) => return None,
            })
        })
    }

    /// The row with this kind, when it is laid out.
    pub fn row(&self, kind: RowKind) -> Option<&ProjectRow> {
        self.rows.iter().find(|row| row.kind == kind)
    }
}

/// A header row with its buttons, laid out from the right: `+`, then close,
/// then refresh (the last two only on an explicit project; refresh only while
/// nothing is being scanned).
fn header_row(index: usize, rect: Rect, project: Option<&ProjectView>) -> ProjectRow {
    let top = rect.origin.y + (rect.size.y - PLUS_SIZE) / 2.0;
    let plus_x = rect.origin.x + rect.size.x - ROW_PAD_RIGHT / 2.0 - PLUS_SIZE;
    let button = |x: f32| Rect::xywh(x, top, PLUS_SIZE, PLUS_SIZE);
    let closable = project.is_some_and(|project| project.closable);
    let close = closable.then(|| button(plus_x - BUTTON_GAP - PLUS_SIZE));
    let refresh = (closable
        && project.is_some_and(|project| project.scan == ProjectScanState::Idle))
    .then(|| button(plus_x - (BUTTON_GAP + PLUS_SIZE) * 2.0));
    ProjectRow {
        kind: RowKind::Header(index),
        rect,
        plus: Some(button(plus_x)),
        refresh,
        close,
    }
}

#[cfg(test)]
#[path = "projects_folders_tests.rs"]
mod projects_folders_tests;
