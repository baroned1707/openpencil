//! Projects dock arm — the native host's platform tail.
//!
//! Where the dock sits (`host_canvas_geometry::projects_dock_rect`, the same
//! rect the canvas and the property panel stop short of), what a press /
//! hover / wheel means (`projects_dock_flow`) and how it paints
//! (`ProjectsDock`) all live in `op-editor-ui`. This file supplies the
//! frame, the repaint marks and the "not while presenting" guard.

use super::press_ctx::PressCtx;
use super::WidgetHostNative;
use op_editor_ui::widgets::projects_dock_flow::{self as dock_flow, DockPress};
use op_editor_ui::widgets::{PaintCx, ProjectsDockLayout};
use op_editor_ui::Point2D;

use crate::backend::NativeFrameBackend;

impl WidgetHostNative {
    /// The dock's layout, or `None` while it is not shown (unsupported host,
    /// touch chrome, no projects, or a presentation owning the window).
    pub(in crate::widget_host) fn projects_dock_layout(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<ProjectsDockLayout> {
        if self.preview_slideshow_active() {
            return None;
        }
        dock_flow::layout(&self.editor_state, viewport_w, viewport_h)
    }

    /// Paint the dock. Called after the property panel so the far right
    /// edge reads as the dock's column.
    pub(in crate::widget_host) fn paint_projects_dock(
        &self,
        frame: &mut NativeFrameBackend<'_>,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        let Some(layout) = self.projects_dock_layout(viewport_w, viewport_h) else {
            return;
        };
        let text = op_editor_ui::widgets::projects_panel_flow::text(&self.editor_state);
        let title = dock_flow::title(&self.editor_state);
        let widget = dock_flow::widget(&self.editor_state, &text, title);
        let mut cx = PaintCx { backend: frame };
        widget.paint(&mut cx, &layout, &self.theme);
    }

    /// Press tier: the dock owns a click on its column unless a surface
    /// painted above it (Git panel, chat card, model picker) covers the point.
    /// `None` falls through to the tiers below.
    pub(in crate::widget_host) fn press_projects_dock_tier(
        &mut self,
        ctx: &PressCtx,
    ) -> Option<bool> {
        let (x, y) = (ctx.x, ctx.y);
        if ctx.in_git_panel
            || ctx.in_chat_model_picker
            || self.chat_panel_surface_contains(x, y, ctx.viewport_width, ctx.viewport_height)
        {
            return None;
        }
        let layout = self.projects_dock_layout(ctx.viewport_width, ctx.viewport_height)?;
        match dock_flow::press(&mut self.editor_state, &layout, Point2D::new(x, y)) {
            DockPress::Missed => None,
            DockPress::Claimed => {
                self.mark_dirty();
                Some(true)
            }
        }
    }

    /// Hover tier: `Some(repaint)` when the cursor is on the dock and no
    /// higher surface owns the point. Off the dock — or under the chat card —
    /// any hover the dock still holds is dropped and the ladder moves on.
    pub(in crate::widget_host) fn cursor_move_projects_dock_tier(
        &mut self,
        x: f32,
        y: f32,
        chat_owns_point: bool,
    ) -> Option<bool> {
        let layout = self
            .projects_dock_layout(self.last_viewport_w, self.last_viewport_h)
            .filter(|layout| !chat_owns_point && layout.contains_point(Point2D::new(x, y)));
        let Some(layout) = layout else {
            if dock_flow::clear_hover(&mut self.editor_state) {
                self.mark_dirty();
            }
            return None;
        };
        let changed = dock_flow::cursor_move(&mut self.editor_state, &layout, Point2D::new(x, y));
        // A cursor that arrives from the canvas leaves lower-layer hovers
        // behind; drop them so nothing glows under the dock.
        let cleared = self.clear_lower_overlay_hover();
        if changed {
            self.mark_dirty();
        }
        Some(changed || cleared)
    }

    /// Wheel over the dock: `Some(changed)` when the point is on it.
    pub(in crate::widget_host) fn try_scroll_projects_dock(
        &mut self,
        x: f32,
        y: f32,
        delta_y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        let layout = self.projects_dock_layout(viewport_w, viewport_h);
        let Some(changed) = dock_flow::scroll(
            &mut self.editor_state,
            layout.as_ref(),
            Point2D::new(x, y),
            delta_y,
        ) else {
            return false;
        };
        if changed {
            self.mark_dirty();
        }
        true
    }
}

#[cfg(test)]
#[path = "projects_dock_tests.rs"]
mod projects_dock_tests;
