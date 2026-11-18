//! Geometry of the Projects dock: its header, its collapse chevron and the
//! list body, for a dock rect the shared canvas geometry already placed.
//!
//! The dock's OUTER rect is not computed here — `host_canvas_geometry::
//! projects_dock_rect` owns it, because the canvas, the property panel and
//! every floating anchor must agree on it. This module only splits that rect
//! into the parts paint and hit-test share, so a painted chevron and its
//! click target can never drift apart.

use op_editor_core::{DocumentTabView, ProjectRowTarget, ProjectView};

use crate::widgets::projects_panel_layout::ProjectsPanelLayout;
use crate::{Point2D, Rect};

/// Width of the open dock.
pub const DOCK_WIDTH: f32 = 232.0;
/// Width of the slim strip a collapsed dock leaves at the window edge.
pub const DOCK_COLLAPSED_WIDTH: f32 = 28.0;
/// Height of the open dock's header (title + collapse chevron).
pub const DOCK_HEADER_HEIGHT: f32 = 36.0;
/// Side of the square collapse chevron button in the open header.
pub const DOCK_TOGGLE_SIZE: f32 = 24.0;
/// Gap between the chevron button and the dock's right edge.
const TOGGLE_MARGIN_RIGHT: f32 = 8.0;
/// Gap between the "Open project" button and the chevron beside it.
const OPEN_BUTTON_GAP: f32 = 2.0;
/// Height of each of the two targets (fold chevron, "Open project") the empty
/// dock's slim strip stacks.
pub const STRIP_TARGET_HEIGHT: f32 = 36.0;

/// What a point on the dock lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// The collapse chevron (open dock) or anywhere on the strip (collapsed).
    Toggle,
    /// The "Open project" button: in the open header, or the folder-plus glyph
    /// of the empty dock's strip.
    OpenProject,
    /// The open dock's header, off the chevron.
    Header,
    /// The list body; `Some` when it landed on a row.
    Body(Option<ProjectRowTarget>),
}

/// Where the dock's parts sit.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectsDockLayout {
    /// The whole dock.
    pub rect: Rect,
    pub open: bool,
    /// The open dock's header band; empty when collapsed.
    pub header: Rect,
    /// The chevron's hit rect.
    pub toggle: Rect,
    /// The "Open project" button; `None` on a strip that has projects (it
    /// folds out first).
    pub open_project: Option<Rect>,
    /// The project list; `None` when collapsed.
    pub list: Option<ProjectsPanelLayout>,
}

impl ProjectsDockLayout {
    /// Split `rect` for the dock in the given open state, with its project
    /// list scrolled by `scroll_offset`.
    pub fn new(projects: &[ProjectView], rect: Rect, open: bool, scroll_offset: f32) -> Self {
        Self::with_tabs(projects, &[], rect, open, scroll_offset)
    }

    /// [`Self::new`] with the open documents, so the list can show the ones no
    /// project row stands for (`ProjectsPanelLayout::with_tabs`).
    pub fn with_tabs(
        projects: &[ProjectView],
        tabs: &[DocumentTabView],
        rect: Rect,
        open: bool,
        scroll_offset: f32,
    ) -> Self {
        let header_origin = Rect::xywh(rect.origin.x, rect.origin.y, 0.0, 0.0);
        if !open && projects.is_empty() {
            // Nothing to unfold yet: the strip is the place to start, so it
            // stacks the fold chevron over an "Open project" button.
            let target_height = STRIP_TARGET_HEIGHT.min(rect.size.y.max(0.0));
            let below = (rect.size.y - target_height).max(0.0).min(target_height);
            return Self {
                rect,
                open,
                header: header_origin,
                toggle: Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, target_height),
                open_project: Some(Rect::xywh(
                    rect.origin.x,
                    rect.origin.y + target_height,
                    rect.size.x,
                    below,
                )),
                list: None,
            };
        }
        if !open {
            return Self {
                rect,
                open,
                header: header_origin,
                // The whole strip is the target: 28 px is too slim to aim
                // a chevron-sized button at.
                toggle: rect,
                open_project: None,
                list: None,
            };
        }
        let header_height = DOCK_HEADER_HEIGHT.min(rect.size.y.max(0.0));
        let header = Rect::xywh(rect.origin.x, rect.origin.y, rect.size.x, header_height);
        let toggle = Rect::xywh(
            rect.origin.x + rect.size.x - TOGGLE_MARGIN_RIGHT - DOCK_TOGGLE_SIZE,
            rect.origin.y + (header_height - DOCK_TOGGLE_SIZE) / 2.0,
            DOCK_TOGGLE_SIZE,
            DOCK_TOGGLE_SIZE,
        );
        let open_project = Rect::xywh(
            toggle.origin.x - OPEN_BUTTON_GAP - DOCK_TOGGLE_SIZE,
            toggle.origin.y,
            DOCK_TOGGLE_SIZE,
            DOCK_TOGGLE_SIZE,
        );
        let body = Rect::xywh(
            rect.origin.x,
            rect.origin.y + header_height,
            rect.size.x,
            (rect.size.y - header_height).max(0.0),
        );
        Self {
            rect,
            open,
            header,
            toggle,
            open_project: Some(open_project),
            list: Some(ProjectsPanelLayout::with_tabs(
                projects,
                tabs,
                body,
                scroll_offset,
            )),
        }
    }

    /// Whether `point` is on the dock — the dock's whole claim.
    pub fn contains_point(&self, point: Point2D) -> bool {
        self.rect.contains(point)
    }

    /// What `point` lands on, or `None` off the dock.
    pub fn hit(&self, point: Point2D) -> Option<DockHit> {
        if !self.contains_point(point) {
            return None;
        }
        if self
            .open_project
            .is_some_and(|button| button.contains(point))
        {
            return Some(DockHit::OpenProject);
        }
        if self.toggle.contains(point) {
            return Some(DockHit::Toggle);
        }
        match &self.list {
            Some(list) if list.contains_point(point) => Some(DockHit::Body(list.hit(point))),
            _ => Some(DockHit::Header),
        }
    }
}
