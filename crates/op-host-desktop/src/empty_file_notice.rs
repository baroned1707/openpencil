//! The one-time notice after an empty `.op` file was opened as a blank
//! document: either it was formatted on disk, or it could not be written and
//! the tab is unsaved until the user saves (or Saves As elsewhere).

use std::path::Path;

use op_editor_core::editor_toast::EditorToastLevel;
use op_host_services::doc_io::EmptyFileOutcome;

use crate::DesktopApp;

/// Locale key: the empty file was formatted as a blank `.op` document.
pub(crate) const EMPTY_FILE_INITIALIZED_KEY: &str = "documents.emptyFileInitialized";
/// Locale key: the blank document is open but the file could not be written.
pub(crate) const EMPTY_FILE_NOT_WRITABLE_KEY: &str = "documents.emptyFileNotWritable";

impl DesktopApp {
    /// Tell the user what happened to the empty file `path`, on the document
    /// that is now on screen.
    pub(crate) fn announce_empty_file_open(&mut self, path: &Path, outcome: &EmptyFileOutcome) {
        let (key, level) = match outcome {
            EmptyFileOutcome::Formatted => (EMPTY_FILE_INITIALIZED_KEY, EditorToastLevel::Info),
            EmptyFileOutcome::NotWritable(_) => {
                (EMPTY_FILE_NOT_WRITABLE_KEY, EditorToastLevel::Warn)
            }
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            key,
            vec![("name".to_string(), name)],
            level,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }
}

#[cfg(test)]
#[path = "empty_file_notice_tests.rs"]
mod tests;
