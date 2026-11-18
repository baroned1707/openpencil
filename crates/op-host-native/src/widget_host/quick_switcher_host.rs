//! Platform tail of the Cmd+K quick switcher.
//!
//! What the switcher lists, how it ranks and what its keys do live in
//! `op-editor-core` (`quick_switcher*`); how it places, paints and takes a
//! press lives in `op-editor-ui` (`widgets::quick_switcher*`). This file adds
//! what only the host can do: refuse to open over another modal or a focused
//! text field, paint the card above the chrome, and make an open switcher
//! swallow every pointer event so nothing under it reacts.

use super::WidgetHostNative;
use crate::backend::NativeFrameBackend;
use op_editor_core::quick_switcher_keyboard as keyboard;
use op_editor_ui::widgets::quick_switcher_flow as flow;
use op_editor_ui::widgets::PaintCx;
use op_editor_ui::Point2D;

impl WidgetHostNative {
    /// Whether the quick switcher is open and owns the keyboard and the pointer.
    pub fn quick_switcher_active(&self) -> bool {
        keyboard::is_active(&self.editor_state)
    }

    /// A modal or dropdown the switcher must not open over. A text field that
    /// owns the keyboard is checked separately (`input_active`).
    fn quick_switcher_blocked_by_surface(&self) -> bool {
        let ui = &self.editor_state.editor_ui;
        ui.agent_settings_open
            || ui.collab.panel.open
            || ui.login_modal_open
            || ui.file_menu_open
            || ui.export_quick_menu_open
            || ui.slides_panel.export_menu_open
            || ui.export_dialog_open
            || ui.figma_import_open
            || ui.account_menu_open
            || ui.import_menu_open
            || ui.locale_picker.open
            || ui.projects_panel.info_bar.page_menu.open
            || ui.missing_fonts_modal_open
            || ui.layer_context_menu.is_some()
            || ui.shape_picker.open
    }

    /// Cmd+K: close the switcher when it is open, else open it. Reports whether
    /// the chord was used; `false` leaves it to its ordinary meaning (a text
    /// field owns the keyboard, another modal is up, the host has no dock).
    pub fn apply_toggle_quick_switcher(&mut self) -> bool {
        if self.quick_switcher_active() {
            keyboard::close(&mut self.editor_state);
            self.mark_dirty();
            return true;
        }
        if self.input_active() || self.quick_switcher_blocked_by_surface() {
            return false;
        }
        if !keyboard::open(&mut self.editor_state, self.now_ms) {
            return false;
        }
        // The cursor may be resting on something that hovers beneath the card.
        self.clear_lower_overlay_hover();
        self.mark_dirty();
        true
    }

    /// Up / Down: move the highlighted row (`delta` is `-1` or `1`).
    pub fn apply_quick_switcher_move(&mut self, delta: isize) -> bool {
        let moved = keyboard::move_selection(&mut self.editor_state, delta);
        if moved {
            self.mark_dirty();
        }
        moved
    }

    /// Enter: choose the highlighted row. `true` whenever the switcher owned
    /// the key, even with nothing to choose.
    pub fn apply_quick_switcher_confirm(&mut self) -> bool {
        let Some(_) = keyboard::confirm(&mut self.editor_state) else {
            return false;
        };
        self.mark_dirty();
        true
    }

    /// Escape: close without choosing.
    pub fn apply_quick_switcher_cancel(&mut self) -> bool {
        let closed = keyboard::cancel(&mut self.editor_state);
        if closed {
            self.mark_dirty();
        }
        closed
    }

    /// Backspace in the query.
    pub fn apply_quick_switcher_backspace(&mut self) -> bool {
        let outcome = keyboard::backspace(&mut self.editor_state, self.now_ms);
        self.apply_quick_switcher_edit(outcome)
    }

    /// Forward delete in the query.
    pub fn apply_quick_switcher_delete(&mut self) -> bool {
        let outcome = keyboard::delete_forward(&mut self.editor_state, self.now_ms);
        self.apply_quick_switcher_edit(outcome)
    }

    /// Left / Right in the query (`extend` grows the selection).
    pub fn apply_quick_switcher_caret(&mut self, forward: bool, extend: bool) -> bool {
        let moved = keyboard::move_caret(&mut self.editor_state, forward, extend, self.now_ms);
        if moved {
            self.mark_dirty();
        }
        moved
    }

    /// Home / End in the query.
    pub fn apply_quick_switcher_edge(&mut self, end: bool, extend: bool) -> bool {
        let moved = keyboard::move_caret_to_edge(&mut self.editor_state, end, extend, self.now_ms);
        if moved {
            self.mark_dirty();
        }
        moved
    }

    /// `Some(owned)` while the switcher is open (the key is always its own);
    /// `None` when it is closed.
    fn apply_quick_switcher_edit(&mut self, outcome: Option<bool>) -> bool {
        if outcome == Some(true) {
            self.mark_dirty();
        }
        outcome.is_some()
    }

    /// Paint the card above the chrome (a no-op while closed).
    pub(in crate::widget_host) fn paint_quick_switcher(
        &self,
        frame: &mut NativeFrameBackend<'_>,
        viewport_width: f32,
    ) {
        let mut cx = PaintCx { backend: frame };
        flow::paint(&mut cx, &self.editor_state, viewport_width, self.now_ms);
    }

    /// Press tier: an open switcher claims every press.
    pub(in crate::widget_host) fn press_quick_switcher(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
    ) -> Option<bool> {
        if !flow::press(&mut self.editor_state, viewport_width, Point2D::new(x, y)) {
            return None;
        }
        self.mark_dirty();
        Some(true)
    }

    /// Hover tier: an open switcher owns the cursor, so no hover forms beneath it.
    pub(in crate::widget_host) fn cursor_move_quick_switcher(
        &mut self,
        x: f32,
        y: f32,
    ) -> Option<bool> {
        let viewport_width = self.last_viewport_w;
        let changed =
            flow::cursor_move(&mut self.editor_state, viewport_width, Point2D::new(x, y))?;
        let cleared = self.clear_lower_overlay_hover();
        if changed || cleared {
            self.mark_dirty();
        }
        Some(changed || cleared)
    }

    /// Wheel tier: an open switcher swallows the wheel (and scrolls its list).
    pub(in crate::widget_host) fn scroll_quick_switcher(&mut self, delta_y: f32) -> Option<bool> {
        let changed = flow::scroll(&mut self.editor_state, delta_y)?;
        if changed {
            self.mark_dirty();
        }
        Some(true)
    }
}
