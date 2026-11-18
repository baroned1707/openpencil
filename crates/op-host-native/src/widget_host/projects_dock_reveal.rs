//! Scroll one file's row of the Projects dock into view, for the info bar's
//! breadcrumb. The desktop host unfolds whatever hides the row first, then calls
//! this once the rebuilt rows have a layout.

use super::WidgetHostNative;
use op_editor_ui::widgets::projects_panel_flow::reveal_file;

impl WidgetHostNative {
    /// Scroll the row of the file at `path` fully into view. `true` when the
    /// list moved; `false` when the row is already visible, the dock is folded
    /// away, or the file has no row.
    pub fn reveal_project_file(&mut self, path: &str) -> bool {
        let Some(dock) = self.projects_dock_layout(self.last_viewport_w, self.last_viewport_h)
        else {
            return false;
        };
        let Some(list) = dock.list.as_ref() else {
            return false;
        };
        let moved = reveal_file(&mut self.editor_state, list, path);
        if moved {
            self.mark_dirty();
        }
        moved
    }
}
