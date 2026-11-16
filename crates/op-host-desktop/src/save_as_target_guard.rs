//! A file belongs to one tab. Binding a second tab to it (Save As onto a file
//! another tab has open) would let the later save silently overwrite what the
//! earlier tab saved, so every bind goes through this check first.

use std::path::Path;

use op_editor_core::editor_toast::EditorToastLevel;

use crate::DesktopApp;

/// Shown when a Save As target is a file another tab has open. `{{name}}` is
/// that tab's label.
pub(crate) const SAVE_AS_ALREADY_OPEN_KEY: &str = "documents.saveAsAlreadyOpen";

impl DesktopApp {
    /// The tab, other than the one on screen, that is bound to `path`.
    /// Saving the on-screen tab to its own file is not a conflict.
    pub(crate) fn other_tab_bound_to(&mut self, path: &Path) -> Option<usize> {
        self.sync_active_slot();
        self.documents
            .index_of_path(path)
            .filter(|&index| index != self.documents.active_index())
    }

    /// Refuse a Save As onto a file another tab has open: tell the user which
    /// tab holds it and write nothing. Returns whether it was refused.
    pub(crate) fn refuse_save_as_onto_file_open_in_another_tab(&mut self, path: &Path) -> bool {
        let Some(index) = self.other_tab_bound_to(path) else {
            return false;
        };
        eprintln!(
            "[save] refused Save As onto {}: open in another tab",
            path.display()
        );
        let name = self.slot_display_name(index);
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            SAVE_AS_ALREADY_OPEN_KEY,
            vec![("name".to_string(), name)],
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
        true
    }
}

#[cfg(test)]
#[path = "save_as_target_guard_tests.rs"]
mod tests;
