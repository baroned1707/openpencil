//! Platform tail of the info bar's page menu (the dropdown under the page chip).
//!
//! What the menu lists and what a press on it does live in `op-editor-ui`
//! (`widgets::page_menu*`, with the shared `switch_page` helper); the open flag
//! and hover are `ProjectsPanelState.info_bar`. This file adds what only the
//! host can do: anchor the menu on the chip as paint measured it, paint it above
//! the chrome, take the press before anything underneath, and re-fit the viewport
//! after a page switch. Native only: the web has no page chip in v1.

use super::WidgetHostNative;
use crate::backend::NativeFrameBackend;
use op_editor_core::{PageSwitch, TopBarButton};
use op_editor_ui::widgets::page_menu_flow::{self as flow, PageMenuPress};
use op_editor_ui::widgets::{PaintCx, TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

impl WidgetHostNative {
    /// Whether the page menu is open.
    pub fn page_menu_open(&self) -> bool {
        self.editor_state
            .editor_ui
            .projects_panel
            .info_bar
            .page_menu
            .open
    }

    /// The page chip as the bar paints it (family-aware metric), `None` when
    /// the bar shows none (one page, a narrow window, the web).
    fn page_chip_rect(&self, viewport_width: f32) -> Option<Rect> {
        let bar = TopBar::for_editor_state(&self.editor_state);
        let rect = Rect::xywh(0.0, 0.0, viewport_width, TOP_BAR_HEIGHT);
        self.topbar_button_rect(&bar, rect, TopBarButton::PageChip)
    }

    fn page_menu_viewport(&self) -> Rect {
        Rect::xywh(0.0, 0.0, self.last_viewport_w, self.last_viewport_h)
    }

    /// The open menu's footprint, for hover / cursor / wheel ownership.
    pub(in crate::widget_host) fn page_menu_popup_rect(&self) -> Option<Rect> {
        let anchor = self.page_chip_rect(self.last_viewport_w);
        flow::popup_rect(&self.editor_state, anchor, self.page_menu_viewport())
    }

    /// Paint the menu above the chrome (a no-op while closed).
    pub(in crate::widget_host) fn paint_page_menu(
        &self,
        frame: &mut NativeFrameBackend<'_>,
        viewport_width: f32,
        viewport_height: f32,
    ) {
        if !self.page_menu_open() {
            return;
        }
        let anchor = self.page_chip_rect(viewport_width);
        let viewport = Rect::xywh(0.0, 0.0, viewport_width, viewport_height);
        let mut cx = PaintCx { backend: frame };
        flow::paint(&mut cx, &self.editor_state, anchor, viewport);
    }

    /// Press tier: an open menu takes every press (a row switches the page, any
    /// other press closes it). `None` while closed.
    pub(in crate::widget_host) fn press_page_menu(&mut self, x: f32, y: f32) -> Option<bool> {
        if !self.page_menu_open() {
            return None;
        }
        let anchor = self.page_chip_rect(self.last_viewport_w);
        let viewport = self.page_menu_viewport();
        let outcome = flow::press(&mut self.editor_state, anchor, viewport, Point2D::new(x, y));
        match outcome {
            PageMenuPress::NotOpen => return None,
            PageMenuPress::Chose(PageSwitch::Refit) => {
                self.fit_active_page_after_switch(self.last_viewport_w, self.last_viewport_h);
            }
            PageMenuPress::Chose(PageSwitch::Preserved)
            | PageMenuPress::Swallowed
            | PageMenuPress::Dismissed => {}
        }
        self.mark_dirty();
        Some(true)
    }

    /// Hover tier: rows highlight under the cursor. Only a cursor over the menu
    /// is claimed; elsewhere the chrome below keeps its own hover.
    pub(in crate::widget_host) fn cursor_move_page_menu(&mut self, x: f32, y: f32) -> Option<bool> {
        let anchor = self.page_chip_rect(self.last_viewport_w);
        let viewport = self.page_menu_viewport();
        let point = Point2D::new(x, y);
        let over = flow::popup_rect(&self.editor_state, anchor, viewport)
            .is_some_and(|popup| popup.contains(point));
        let changed = flow::cursor_move(&mut self.editor_state, anchor, viewport, point)?;
        if changed {
            self.mark_dirty();
        }
        over.then_some(changed)
    }

    /// Wheel tier: scrolls the list while the pointer is over the menu.
    pub(in crate::widget_host) fn scroll_page_menu(
        &mut self,
        x: f32,
        y: f32,
        delta_y: f32,
    ) -> bool {
        let Some(popup) = self.page_menu_popup_rect() else {
            return false;
        };
        if !popup.contains(Point2D::new(x, y)) {
            return false;
        }
        if flow::scroll(&mut self.editor_state, delta_y).unwrap_or(false) {
            self.mark_dirty();
        }
        true
    }

    /// Escape: close the menu. `true` when it was open.
    pub(in crate::widget_host) fn escape_page_menu(&mut self) -> bool {
        let closed = flow::escape(&mut self.editor_state);
        if closed {
            self.mark_dirty();
        }
        closed
    }
}

#[cfg(test)]
#[path = "info_bar_host_tests.rs"]
mod info_bar_host_tests;
