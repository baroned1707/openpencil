//! Geometry of the Projects tab: where every row sits for a given scroll
//! offset, and what a point lands on.
//!
//! One pure function of the project rows, the rail body rect and the scroll
//! offset. Paint, hit-test and scroll all read the same [`ProjectsPanelLayout`],
//! so a painted row and its click target can never drift apart.

use op_editor_core::{ProjectRowTarget, ProjectView};

use crate::{Point2D, Rect};

/// Height of a project header row.
pub const HEADER_HEIGHT: f32 = 32.0;
/// Height of a file row (and of the empty project's "New design" row).
pub const FILE_HEIGHT: f32 = 28.0;
/// Side of the header's square `+` button.
pub const PLUS_SIZE: f32 = 22.0;
/// Space above the first row and below the last.
const LIST_PAD_TOP: f32 = 4.0;
const LIST_PAD_BOTTOM: f32 = 12.0;
/// Air above every project header but the first, so each project reads as
/// one group instead of a flat list of rows.
const PROJECT_GAP: f32 = 6.0;
/// Rows are washed this far in from the rail's edges, so a selected row
/// reads as a block resting on the rail rather than a stripe through it.
pub const ROW_INSET_X: f32 = 8.0;
/// Right padding inside a row for its trailing content.
pub const ROW_PAD_RIGHT: f32 = 8.0;

/// What a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Project header, by project index.
    Header(usize),
    /// File row, by `(project index, file index)`.
    File(usize, usize),
    /// The "New design" row an empty, expanded project shows.
    NewDesign(usize),
}

/// One laid-out row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectRow {
    pub kind: RowKind,
    /// On-screen rect after scrolling (may lie outside the viewport).
    pub rect: Rect,
    /// A header's `+` button; `None` on every other row.
    pub plus: Option<Rect>,
}

/// Where the Projects tab's rows sit.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectsPanelLayout {
    /// The viewport: the rail body below the tab row.
    pub panel: Rect,
    pub rows: Vec<ProjectRow>,
    /// Total height of all rows plus padding.
    pub content_height: f32,
    /// The scroll offset the rows were placed with, already clamped.
    pub offset: f32,
    /// True when there are no projects to list at all.
    pub empty: bool,
}

impl ProjectsPanelLayout {
    /// Lay `projects` out inside `panel` scrolled by `scroll_offset`.
    ///
    /// A collapsed project contributes only its header. An expanded one
    /// with no files contributes a "New design" row instead. A stale
    /// offset (the list shrank) is clamped, so the rows never float above
    /// the viewport.
    pub fn new(projects: &[ProjectView], panel: Rect, scroll_offset: f32) -> Self {
        // `(kind, height, space above)` in paint order.
        let mut specs: Vec<(RowKind, f32, f32)> = Vec::new();
        for (index, project) in projects.iter().enumerate() {
            let gap = if index == 0 { 0.0 } else { PROJECT_GAP };
            specs.push((RowKind::Header(index), HEADER_HEIGHT, gap));
            if !project.expanded {
                continue;
            }
            if project.files.is_empty() {
                specs.push((RowKind::NewDesign(index), FILE_HEIGHT, 0.0));
            }
            for file in 0..project.files.len() {
                specs.push((RowKind::File(index, file), FILE_HEIGHT, 0.0));
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
                let plus = matches!(kind, RowKind::Header(_)).then(|| {
                    Rect::xywh(
                        rect.origin.x + rect.size.x - ROW_PAD_RIGHT / 2.0 - PLUS_SIZE,
                        rect.origin.y + (height - PLUS_SIZE) / 2.0,
                        PLUS_SIZE,
                        PLUS_SIZE,
                    )
                });
                ProjectRow { kind, rect, plus }
            })
            .collect();
        Self {
            panel,
            rows,
            content_height,
            offset,
            empty: projects.is_empty(),
        }
    }

    /// How far the list can scroll.
    pub fn max_scroll(&self) -> f32 {
        (self.content_height - self.panel.size.y).max(0.0)
    }

    /// Whether `point` is on the rail body (the tab's whole claim).
    pub fn contains_point(&self, point: Point2D) -> bool {
        self.panel.contains(point)
    }

    /// What `point` lands on, or `None` for the gaps and anywhere off the
    /// viewport. A header's `+` wins over the header it sits on.
    ///
    /// The `+` is hit-testable wherever it would be painted, hovered or
    /// not: a pointer always moves onto a header before it presses it, so
    /// the button is already showing by the time it can be clicked.
    pub fn hit(&self, point: Point2D) -> Option<ProjectRowTarget> {
        if !self.contains_point(point) {
            return None;
        }
        self.rows.iter().find_map(|row| {
            if !row.rect.contains(point) {
                return None;
            }
            Some(match row.kind {
                RowKind::Header(project) => {
                    if row.plus.is_some_and(|plus| plus.contains(point)) {
                        ProjectRowTarget::Plus(project)
                    } else {
                        ProjectRowTarget::Header(project)
                    }
                }
                RowKind::File(project, file) => ProjectRowTarget::File(project, file),
                RowKind::NewDesign(project) => ProjectRowTarget::NewDesignRow(project),
            })
        })
    }

    /// The row with this kind, when it is laid out.
    pub fn row(&self, kind: RowKind) -> Option<&ProjectRow> {
        self.rows.iter().find(|row| row.kind == kind)
    }
}
