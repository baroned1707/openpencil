//! The document-owned slice of [`EditorState`] — what travels with one open
//! document when the user switches tabs.
//!
//! `EditorState` mixes two kinds of data. Document data (the node tree, the
//! selection, undo history, revision counters, per-document view state) belongs
//! to one document. App data (theme, locale, agent connections, clipboard, the
//! chat panel, UIKits, window chrome, ...) belongs to the running app and must
//! survive a switch untouched. A [`DocumentScope`] holds exactly the first kind.
//!
//! [`EditorState::exchange_document_scope`] swaps the live document with a
//! parked one. Every field of `EditorState` is named in it, with no `..` rest
//! pattern: adding a field to `EditorState` (or `EditorUiState`) does not
//! compile until the author classifies it as document (swap it) or app
//! (`name: _`). That is the safety net against a new field silently leaking
//! from one document into another.
//!
//! Everything here is plain data and wasm32-clean.

use crate::chat_sessions::ChatSessions;
use crate::codegen::CodegenState;
use crate::components::ComponentLibrary;
use crate::editor_ui_state::EditorUiDocumentScope;
use crate::history::History;
use crate::selection::SelectionState;
use crate::state::EditorState;
use crate::tool::Tool;
use crate::ui_draft::UiDraftState;
use crate::viewport::Viewport;

/// One entry of the persisted open-document list (session restore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDocument {
    /// Absolute file path of the open document.
    pub path: String,
    /// Whether this was the document on screen.
    pub active: bool,
}

/// Everything about one document that is not app-level state.
#[derive(Debug, Clone)]
pub struct DocumentScope {
    pub(crate) doc: jian_ops_schema::PenDocument,
    pub(crate) selection: SelectionState,
    pub(crate) tool: Tool,
    pub(crate) viewport: Viewport,
    pub(crate) history: History,
    pub(crate) revision: u64,
    pub(crate) saved_revision: u64,
    pub(crate) revision_counter: u64,
    pub(crate) document_generation: u64,
    pub(crate) history_push_count: u64,
    pub(crate) ui: UiDraftState,
    pub(crate) editor_ui: EditorUiDocumentScope,
    /// The document's chat tabs, transcripts and drafts. In memory only: they
    /// are never written to disk or into the `.op` file. The app-level part
    /// of each tab follows the user instead (see `ChatAppPrefs`).
    pub(crate) chat: ChatSessions,
    pub(crate) codegen: CodegenState,
    pub(crate) components: ComponentLibrary,
    pub(crate) app_state_owner: std::collections::BTreeMap<String, usize>,
    pub(crate) instance_write_virtual_anchor: Option<crate::NodeId>,
}

impl DocumentScope {
    /// Take the document-owned fields out of a whole state. App-level fields
    /// are dropped (`name: _`), so the caller must have copied anything it
    /// wants to keep from them first.
    pub fn from_state(state: EditorState) -> Self {
        let EditorState {
            doc,
            selection,
            tool,
            viewport,
            history,
            revision,
            saved_revision,
            revision_counter,
            document_generation,
            history_push_count,
            // App-level: the clipboard deliberately crosses documents.
            clipboard: _,
            ui,
            editor_ui,
            chat,
            codegen,
            components,
            ui_kits: _,
            theme_presets: _,
            theme_presets_dirty: _,
            app_state_owner,
            instance_write_virtual_anchor,
        } = state;
        Self {
            doc,
            selection,
            tool,
            viewport,
            history,
            revision,
            saved_revision,
            revision_counter,
            document_generation,
            history_push_count,
            ui,
            editor_ui: EditorUiDocumentScope::from_state(editor_ui),
            chat,
            codegen,
            components,
            app_state_owner,
            instance_write_virtual_anchor,
        }
    }

    /// True when this document changed since its last saved baseline.
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Generation of this document (see `EditorState::document_generation`).
    pub fn document_generation(&self) -> u64 {
        self.document_generation
    }

    /// Revision of this document's content.
    pub fn document_revision(&self) -> u64 {
        self.revision
    }

    /// Whether the document is exactly as freshly created: no edits ever,
    /// nothing to undo and an untouched chat. Used to reuse a blank untitled tab instead of
    /// stacking a new one beside it.
    pub fn is_pristine(&self) -> bool {
        self.revision == 0
            && self.saved_revision == 0
            && !self.history.can_undo()
            && self.chat.is_untouched()
    }

    /// The name painted for this document, when it has one.
    pub fn file_name_display(&self) -> Option<&str> {
        self.editor_ui.file_name_display.as_deref()
    }

    /// Record the name painted for this document (after a save elsewhere).
    pub fn set_file_name_display(&mut self, name: Option<String>) {
        self.editor_ui.file_name_display = name;
    }

    /// Mark `revision` saved iff `generation` still names this document. The
    /// counterpart of [`EditorState::mark_saved_revision_at`] for a parked
    /// document, so a save acknowledgement can reach the document that issued
    /// it even after the user switched away.
    pub fn mark_saved_revision_at(&mut self, generation: u64, revision: u64) -> bool {
        if generation != self.document_generation {
            return false;
        }
        self.saved_revision = revision;
        self.editor_ui.document_dirty = self.is_dirty();
        true
    }

    /// The collaboration display state of this document. Only its session
    /// fields are meaningful here; the app-level ones live in the on-screen
    /// state (see `CollabUiState::exchange_session`).
    pub fn collab(&self) -> &crate::CollabUiState {
        &self.editor_ui.collab
    }

    /// The chat tabs of this document.
    pub fn chat(&self) -> &ChatSessions {
        &self.chat
    }

    /// Mutable chat tabs, for tearing down a run bound to a parked document.
    pub fn chat_mut(&mut self) -> &mut ChatSessions {
        &mut self.chat
    }

    /// The canonical document, for read-only inspection of a parked tab.
    pub fn doc(&self) -> &jian_ops_schema::PenDocument {
        &self.doc
    }
}

impl EditorState {
    /// Swap this state's document-owned fields with `scope`. After the call
    /// `scope` holds the document that was on screen and `self` shows the one
    /// that was parked; every app-level field is left exactly as it was.
    ///
    /// The caller owns the host-side bookkeeping a swap needs (caches, cancel
    /// in-flight gestures); this only moves editor data. The chat moves with
    /// the document exactly as it is, app-level preferences included: this is
    /// the pure exchange a lens needs. A user-visible switch uses
    /// [`Self::switch_document_scope`].
    pub fn exchange_document_scope(&mut self, scope: &mut DocumentScope) {
        let EditorState {
            doc,
            selection,
            tool,
            viewport,
            history,
            revision,
            saved_revision,
            revision_counter,
            document_generation,
            history_push_count,
            clipboard: _,
            ui,
            editor_ui,
            chat,
            codegen,
            components,
            ui_kits: _,
            theme_presets: _,
            theme_presets_dirty: _,
            app_state_owner,
            instance_write_virtual_anchor,
        } = self;
        let DocumentScope {
            doc: scope_doc,
            selection: scope_selection,
            tool: scope_tool,
            viewport: scope_viewport,
            history: scope_history,
            revision: scope_revision,
            saved_revision: scope_saved_revision,
            revision_counter: scope_revision_counter,
            document_generation: scope_document_generation,
            history_push_count: scope_history_push_count,
            ui: scope_ui,
            editor_ui: scope_editor_ui,
            chat: scope_chat,
            codegen: scope_codegen,
            components: scope_components,
            app_state_owner: scope_app_state_owner,
            instance_write_virtual_anchor: scope_instance_write_virtual_anchor,
        } = scope;
        std::mem::swap(doc, scope_doc);
        std::mem::swap(selection, scope_selection);
        std::mem::swap(tool, scope_tool);
        std::mem::swap(viewport, scope_viewport);
        std::mem::swap(history, scope_history);
        std::mem::swap(revision, scope_revision);
        std::mem::swap(saved_revision, scope_saved_revision);
        std::mem::swap(revision_counter, scope_revision_counter);
        std::mem::swap(document_generation, scope_document_generation);
        std::mem::swap(history_push_count, scope_history_push_count);
        std::mem::swap(ui, scope_ui);
        scope_editor_ui.exchange(editor_ui);
        std::mem::swap(chat, scope_chat);
        std::mem::swap(codegen, scope_codegen);
        std::mem::swap(components, scope_components);
        std::mem::swap(app_state_owner, scope_app_state_owner);
        std::mem::swap(
            instance_write_virtual_anchor,
            scope_instance_write_virtual_anchor,
        );
        // `document_dirty` travels with the scope, but it is derived from the
        // revision pair that just changed; recompute rather than trust it.
        self.sync_dirty_flag();
    }
}

impl EditorState {
    /// [`Self::exchange_document_scope`] for a switch the user sees: the
    /// app-level chat preferences (connected models, selection, panel
    /// placement) follow the user onto the arriving document instead of
    /// staying behind in the one that leaves.
    ///
    /// The collaboration session follows its document, with one exception:
    /// mDNS discovery (`Discovering`) is documented as "the current document
    /// is not yet bound", and the host runtime keeps publishing the endpoint
    /// list to whichever document is on screen. It therefore stays with the
    /// user, exactly as the app-wide display state did before sessions moved
    /// with their document. The collaboration app-level fields (availability,
    /// transports, relay region) never move at all, see
    /// `CollabUiState::exchange_session`.
    pub fn switch_document_scope(&mut self, scope: &mut DocumentScope) {
        let prefs = self.chat.app_prefs();
        let discovery_follows_user =
            self.editor_ui.collab.phase == crate::CollabConnectionPhase::Discovering;
        self.exchange_document_scope(scope);
        if discovery_follows_user {
            self.editor_ui
                .collab
                .exchange_session(&mut scope.editor_ui.collab);
        }
        self.chat.apply_app_prefs(&prefs);
    }
}

#[cfg(test)]
#[path = "document_scope_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "document_scope_chat_tests.rs"]
mod chat_tests;

#[cfg(test)]
#[path = "document_scope_collab_tests.rs"]
mod collab_tests;
