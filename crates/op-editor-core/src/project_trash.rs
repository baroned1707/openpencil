//! Moving a design to the Trash from the Projects dock: the two-step
//! confirmation on the row.
//!
//! Pressing a row's trash button does NOT remove anything: it turns that row
//! into a "Move to Trash?" question (no modal) keyed by the file's PATH, like
//! the inline rename, because the host rebuilds the rows whenever the disk
//! changes. Only the question's confirm button records the path in
//! [`ProjectsPanelState::trash_commit`], its own slot beside the rename commit
//! and `pending_request`: a destructive request is never overwritten by another
//! press of the same frame. Escape, a press elsewhere, a rename or the file
//! vanishing all cancel the question.
//!
//! The host re-validates everything before it touches the disk; the dock only
//! offers the question for rows the host marked `removable`.

use crate::editor_ui_state::ProjectsPanelState;

impl ProjectsPanelState {
    /// Ask "Move to Trash?" on the row at `path`. Any rename is dropped first.
    /// `false` when the row is gone or the host did not mark it removable.
    pub fn begin_trash_confirm(&mut self, path: &str) -> bool {
        let removable = self
            .file_at_path(path)
            .is_some_and(|(_, file)| file.removable);
        if !removable {
            return false;
        }
        self.rename = None;
        self.trash_confirm = Some(path.to_string());
        true
    }

    /// Close the question without removing anything (Escape, a press
    /// elsewhere). `true` when one was open.
    pub fn cancel_trash_confirm(&mut self) -> bool {
        self.trash_confirm.take().is_some()
    }

    /// The user confirmed: hand the file to the host and close the question.
    /// `false` when no question was open.
    pub fn confirm_trash(&mut self) -> bool {
        let Some(path) = self.trash_confirm.take() else {
            return false;
        };
        self.trash_commit = Some(path);
        true
    }

    /// Whether the row at `path` is asking.
    pub fn is_trash_confirming(&self, path: &str) -> bool {
        self.trash_confirm.as_deref() == Some(path)
    }

    /// Drop a question whose row can no longer be seen (the file vanished, was
    /// marked not removable, its folder was folded or the dock closed), so an
    /// invisible question never waits for a stray Escape. Returns whether one
    /// was dropped.
    pub fn drop_hidden_trash_confirm(&mut self) -> bool {
        let Some(path) = self.trash_confirm.as_deref() else {
            return false;
        };
        let visible = self.file_at_path(path).is_some_and(|(project, file)| {
            file.removable
                && self.is_open()
                && project.expanded
                && (file.folder.is_empty() || !project.is_folder_collapsed(&file.folder))
        });
        if visible {
            return false;
        }
        self.trash_confirm = None;
        true
    }
}

#[cfg(test)]
#[path = "project_trash_tests.rs"]
mod tests;
