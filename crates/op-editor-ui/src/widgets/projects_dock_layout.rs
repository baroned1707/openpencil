//! Geometry of the Projects dock: its header, its collapse chevron and the
//! list body, for a dock rect the shared canvas geometry already placed.
//!
//! The dock's OUTER rect is not computed here — `host_canvas_geometry::
//! projects_dock_rect` owns it, because the canvas, the property panel and
//! every floating anchor must agree on it. This module only splits that rect
//! into the parts paint and hit-test share, so a painted chevron and its
//! click target can never drift apart.

use op_editor_core::{ProjectRowTarget, ProjectView};

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

/// What a point on the dock lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockHit {
    /// The collapse chevron (open dock) or anywhere on the strip (collapsed).
    Toggle,
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
    /// The project list; `None` when collapsed.
    pub list: Option<ProjectsPanelLayout>,
}

impl ProjectsDockLayout {
    /// Split `rect` for the dock in the given open state, with its project
    /// list scrolled by `scroll_offset`.
    pub fn new(projects: &[ProjectView], rect: Rect, open: bool, scroll_offset: f32) -> Self {
        if !open {
            return Self {
                rect,
                open,
                header: Rect::xywh(rect.origin.x, rect.origin.y, 0.0, 0.0),
                // The whole strip is the target: 28 px is too slim to aim
                // a chevron-sized button at.
                toggle: rect,
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
            list: Some(ProjectsPanelLayout::new(projects, body, scroll_offset)),
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
        if self.toggle.contains(point) {
            return Some(DockHit::Toggle);
        }
        match &self.list {
            Some(list) if list.contains_point(point) => Some(DockHit::Body(list.hit(point))),
            _ => Some(DockHit::Header),
        }
    }
}
