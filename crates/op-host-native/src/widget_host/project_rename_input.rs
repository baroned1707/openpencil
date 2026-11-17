//! Platform tail of the Projects dock's inline file rename.
//!
//! What the field edits and how a name is judged live in `op-editor-core`
//! (`project_rename`, `project_rename_keyboard`); how it paints and how a
//! press reaches it live in `op-editor-ui`. This file adds what only the host
//! can do: take the caret keys, give the rename the press that left it, and
//! start it with every other input blurred.

use super::WidgetHostNative;
use op_editor_core::project_rename_keyboard as keyboard;
use op_editor_ui::widgets::projects_panel_flow::{rename_press, RenamePress};
use op_editor_ui::Point2D;

impl WidgetHostNative {
    /// Whether the Projects dock's file rename is open and owns the keyboard.
    pub fn project_rename_active(&self) -> bool {
        keyboard::is_active(&self.editor_state)
    }

    /// Left / Right while the rename is open: move the caret one character
    /// (`extend` grows the selection). `false` when no rename is open.
    pub fn apply_project_rename_caret(&mut self, forward: bool, extend: bool) -> bool {
        let moved = keyboard::move_caret(&mut self.editor_state, forward, extend, self.now_ms);
        if moved {
            self.mark_dirty();
        }
        moved
    }

    /// Cmd+Left / Cmd+Right while the rename is open: caret to the start or
    /// the end of the name.
    pub fn apply_project_rename_edge(&mut self, end: bool, extend: bool) -> bool {
        let moved = keyboard::move_caret_to_edge(&mut self.editor_state, end, extend, self.now_ms);
        if moved {
            self.mark_dirty();
        }
        moved
    }

    /// A press anywhere: settle an open rename first. A press inside the field
    /// is the field's; any other press commits a valid new name (or drops the
    /// draft) and then goes on to whatever it landed on. Returns whether the
    /// rename was left.
    pub(in crate::widget_host) fn settle_project_rename_for_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        if !self.project_rename_active() {
            return false;
        }
        let layout = self.projects_dock_layout(viewport_w, viewport_h);
        let list = layout.as_ref().and_then(|dock| dock.list.as_ref());
        let left =
            rename_press(&mut self.editor_state, list, Point2D::new(x, y)) == RenamePress::Left;
        if left {
            self.mark_dirty();
        }
        left
    }

    /// A rename just started on the dock: every other text input gives up its
    /// focus (committing what it holds), so the keyboard has one owner.
    pub(in crate::widget_host) fn blur_inputs_for_project_rename(&mut self) {
        if self.project_rename_active() {
            self.blur_text_inputs_on_blank_press();
        }
    }
}

#[cfg(test)]
#[path = "project_rename_input_tests.rs"]
mod project_rename_input_tests;
