//! Platform tail of the Projects dock's inline file rename.
//!
//! What the field edits and how a name is judged live in `op-editor-core`
//! (`project_rename`, `project_rename_keyboard`); how it paints and how a
//! press reaches it live in `op-editor-ui`. This file adds what only the host
//! can do: take the caret keys, give the rename the press that left it, and
//! start it with every other input blurred.

use super::WidgetHostNative;
use op_editor_core::project_rename_keyboard as keyboard;
use op_editor_ui::widgets::projects_panel_flow::{
    rename_press, reveal_file, settle_trash_confirm, RenamePress, TrashPress,
};
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
        let asking = self
            .editor_state
            .editor_ui
            .projects_panel
            .trash_confirm
            .is_some();
        if !self.project_rename_active() && !asking {
            return false;
        }
        let layout = self.projects_dock_layout(viewport_w, viewport_h);
        let list = layout.as_ref().and_then(|dock| dock.list.as_ref());
        let point = Point2D::new(x, y);
        // A "Move to Trash?" question ends with a press outside the dock; a
        // press on the dock is settled by the dock, which knows its buttons.
        let in_dock = layout
            .as_ref()
            .is_some_and(|dock| dock.contains_point(point));
        let cancelled = !in_dock
            && matches!(
                settle_trash_confirm(&mut self.editor_state, list, point),
                TrashPress::Cancelled { .. }
            );
        let left = rename_press(&mut self.editor_state, list, point) == RenamePress::Left;
        if left || cancelled {
            self.mark_dirty();
        }
        left
    }

    /// Start the inline rename of the file at `path` from the host (a design
    /// just created from the dock), scroll its row into view and take the
    /// keyboard from every other input. `false` when the row is not there.
    pub fn begin_project_rename(&mut self, path: &str) -> bool {
        let started = self
            .editor_state
            .editor_ui
            .projects_panel
            .begin_rename(path, self.now_ms);
        if !started {
            return false;
        }
        if let Some(dock) = self.projects_dock_layout(self.last_viewport_w, self.last_viewport_h) {
            if let Some(list) = dock.list.as_ref() {
                reveal_file(&mut self.editor_state, list, path);
            }
        }
        self.blur_inputs_for_project_rename();
        self.mark_dirty();
        true
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
