//! Putting a document back to a remembered state as one undoable step.
//!
//! A host that wants "revert to how it was when X started" keeps an
//! [`EditorSnapshot`] taken with [`EditorState::snapshot_for_history`] (nodes
//! are shared by `Arc` with the undo history, image payloads are `Arc<str>` and
//! never copied). [`EditorState::revert_to_snapshot_with_undo`] installs it
//! again.
//!
//! This is deliberately not `replace_document_with_undo`: that one resets the
//! draft state wholesale (active page back to 0, variable caches emptied) and
//! bumps the document generation, which would make a revert look like a new
//! document to every save acknowledgement. A revert is an edit of the same
//! document, so it behaves like an undo that has its own history entry.

use crate::document_install::reconcile_active_theme;
use crate::history::EditorSnapshot;
use crate::selection::SelectionState;
use crate::state::EditorState;

impl EditorState {
    /// Install `checkpoint` as the live document, recording the state being
    /// replaced as ONE undo step. Earlier undo steps stay (redo is cleared, as
    /// for any new edit), so Cmd+Z brings the replaced state back.
    ///
    /// The revision is freshly allocated, so the document is dirty afterwards
    /// and every revision-keyed cache rebuilds. The generation is unchanged.
    /// Selection is cleared, and drafts or hover state that name nodes of the
    /// replaced document are dropped.
    pub fn revert_to_snapshot_with_undo(&mut self, checkpoint: &EditorSnapshot) {
        let replaced = self.snapshot_for_history();
        self.restore(checkpoint.clone());
        let active_theme = reconcile_active_theme(&self.doc, &self.ui.variables.active_theme);
        self.clear_document_drafts(active_theme);
        self.editor_ui.clear_document_derived();
        self.codegen.reset_for_document_replacement();
        self.instance_write_virtual_anchor = None;
        self.selection = SelectionState::empty();
        // Pushes the replaced state, clears redo and allocates a new revision.
        self.history_push_past(replaced);
        self.sync_dirty_flag();
    }
}
