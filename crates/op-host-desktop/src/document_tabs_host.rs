//! `DesktopApp` side of open-document tabs: opening, switching and closing.
//!
//! `document_registry` keeps the bookkeeping; `WidgetHostNative::swap_document`
//! moves a document on and off the screen. This file joins them with the
//! desktop-only state that belongs to a document (file path, Git session,
//! collaboration fork saves) and with the checks that decide when a switch is
//! safe. Every way of opening a file lands in a new tab here, and the tab
//! strip's requests (`pending_document_request`) are executed once per frame.

use crate::document_registry::{ActivityFlags, BlockReason, EpochOwner, SlotLocalState};
use crate::git_session::GitSession;
use crate::persistence::{self, LoadedDocument};
use crate::DesktopApp;
use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{DocumentTabRequest, EditorState, FileAction};
use op_host_native::widget_host::ParkedDocument;
use op_host_services::doc_io::{ActionOutcome, ErrorKind};
use std::path::PathBuf;

/// Locale key of the notice shown when a switch is refused.
const SWITCH_BLOCKED_KEY: &str = "documents.switchBlocked";

impl DesktopApp {
    /// Why documents cannot be switched, opened or closed right now, if they
    /// cannot. Agent runs, imports and background writers are not yet bound to
    /// the document that started them, so switching under one would hand its
    /// result to whichever document is on screen by then.
    pub(crate) fn document_switch_blocked(&self) -> Option<BlockReason> {
        let state = self.host.editor_state();
        ActivityFlags {
            chat_turn: self.current_chat.is_some() || state.chat.has_streaming_turn(),
            design_run: self.current_design.is_some() || state.chat.agents_running.0 > 0,
            sub_agents: !self.sub_agents.is_empty(),
            codegen: self.current_codegen.is_some(),
            design_md: self.current_design_md.is_some(),
            import: self.current_figma_import.is_some()
                || self.current_html_import.is_some()
                || self.pending_figma_paste.is_some()
                || self.pending_html_paste.is_some(),
            // Status and diff queries are cancelled by the switch itself
            // (`finish_git_rebind`); only writers that change a repository block.
            git_job: self.git_pull_job.is_some()
                || self.git_push_job.is_some()
                || self.git_clone_job.is_some(),
            background_save: self.save_session.is_active(),
            collaboration: persistence::requires_background_save_as_fork(state),
        }
        .blocked_by()
    }

    /// Tell the user a document switch was refused, and why in the log.
    pub(crate) fn refuse_document_switch(&mut self, reason: BlockReason) {
        eprintln!("[tabs] refused document switch: {reason}");
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            SWITCH_BLOCKED_KEY,
            Vec::new(),
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }

    /// Refresh the registry's mirror of the on-screen document's path. Call
    /// before asking the registry anything that depends on paths.
    pub(crate) fn sync_active_slot(&mut self) {
        self.documents.set_active_path(self.current_path.clone());
    }

    /// Run once per frame: execute the tab strip's request, then publish the
    /// tab rows (and the session list) for the next paint. Returns whether
    /// anything changed.
    pub(crate) fn pump_document_tabs(&mut self) -> bool {
        let mut changed = false;
        if let Some(request) = self
            .host
            .editor_state_mut()
            .editor_ui
            .pending_document_request
            .take()
        {
            self.execute_document_request(request);
            changed = true;
        }
        changed | self.publish_document_tabs()
    }

    /// Queue a tab request for the next frame, as a click on the strip does.
    pub(crate) fn request_document_tab(&mut self, request: DocumentTabRequest) {
        self.host
            .editor_state_mut()
            .editor_ui
            .pending_document_request = Some(request);
        self.request_redraw(true);
    }

    fn execute_document_request(&mut self, request: DocumentTabRequest) {
        self.sync_active_slot();
        let index = match request {
            DocumentTabRequest::Switch(id) | DocumentTabRequest::Close(id) => {
                self.documents.index_of(id)
            }
            DocumentTabRequest::SwitchToIndex(index) => Some(index),
        };
        // A stale id (the tab was closed meanwhile) or an out-of-range
        // position finds nothing and is ignored.
        let Some(index) = index.filter(|&index| index < self.documents.len()) else {
            return;
        };
        match request {
            DocumentTabRequest::Close(_) => {
                self.close_document_tab(index);
            }
            DocumentTabRequest::Switch(_) | DocumentTabRequest::SwitchToIndex(_) => {
                self.activate_document_slot(index);
            }
        }
    }

    /// Mirror the registry into `editor_ui.document_tabs` and the persisted
    /// session list. Cheap: nothing is written unless it changed.
    pub(crate) fn publish_document_tabs(&mut self) -> bool {
        self.sync_active_slot();
        let locale = self.host.editor_state().editor_ui.locale;
        let untitled = op_i18n::translate(locale, "common.untitled");
        let tabs = self.documents.tab_views(self.document_is_dirty(), untitled);
        let session = self.documents.session_documents();
        let ui = &self.host.editor_state().editor_ui;
        let tabs_changed = ui.document_tabs != tabs;
        let session_changed = ui.session_documents != session;
        if !tabs_changed && !session_changed {
            return false;
        }
        let before = op_host_services::settings_io::fingerprint(self.host.editor_state());
        let ui = &mut self.host.editor_state_mut().editor_ui;
        ui.document_tabs = tabs;
        ui.session_documents = session;
        op_host_services::settings_io::save_if_changed(self.host.editor_state(), before);
        self.host.mark_editor_state_dirty();
        true
    }

    /// Bring the document at `target` on screen. `true` when it is showing
    /// afterwards (including when it already was).
    pub(crate) fn activate_document_slot(&mut self, target: usize) -> bool {
        self.sync_active_slot();
        if target == self.documents.active_index() {
            return true;
        }
        if target >= self.documents.len() {
            return false;
        }
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return false;
        }
        let Some(mut incoming) = self.documents.take_parked(target) else {
            return false;
        };
        if !self.host.swap_document(&mut incoming) {
            self.documents.restore_parked(target, incoming);
            return false;
        }
        // `incoming` now holds the document that was on screen.
        let git_before = self.git_binding();
        let leaving = self.stash_local_state();
        let arriving = self.documents.take_local(target);
        self.documents.finish_switch(target, incoming, leaving);
        self.adopt_local_state(arriving);
        self.finish_document_switch(git_before);
        true
    }

    /// Move the desktop-side state of the on-screen document into a value the
    /// registry can park, leaving this app's mirror fields empty.
    fn stash_local_state(&mut self) -> SlotLocalState {
        SlotLocalState {
            path: self.current_path.take(),
            git_session: Some(std::mem::take(&mut self.git_session)),
            collab_fork_saves: std::mem::take(&mut self.collab_fork_saves),
        }
    }

    /// Make `local` the on-screen document's desktop-side state.
    fn adopt_local_state(&mut self, local: SlotLocalState) {
        let never_bound = local.git_session.is_none();
        self.git_session = local.git_session.unwrap_or_else(GitSession::new);
        self.current_path = local.path;
        self.collab_fork_saves = local.collab_fork_saves;
        if never_bound {
            self.git_session.rebind(self.current_path.as_deref());
        }
    }

    /// What every document switch ends with: cancel what belonged to the
    /// document that left, point Git and the title at the one that arrived,
    /// and republish the tab strip.
    fn finish_document_switch(&mut self, git_before: (Option<PathBuf>, Option<PathBuf>)) {
        self.pending_cursor_move = None;
        self.image_search.reset();
        // Accessibility actions name nodes of the previous document.
        if let Some(a11y) = self.a11y.as_mut() {
            let _ = a11y.drain_actions();
        }
        let ui = &mut self.host.editor_state_mut().editor_ui;
        // A clone wizard and the image popovers belong to the document that
        // was on screen; their results would otherwise land in this one.
        ui.git_panel.clone_form = None;
        ui.image_panel.close_popovers();
        self.finish_git_rebind(git_before.0, git_before.1);
        self.publish_document_tabs();
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }

    /// Show a freshly built document in a new tab. `path` is the file it is
    /// bound to, if any. The document that was on screen is parked.
    fn install_new_document_tab(
        &mut self,
        mut incoming: ParkedDocument,
        path: Option<PathBuf>,
    ) -> bool {
        self.sync_active_slot();
        if !self.host.swap_document(&mut incoming) {
            return false;
        }
        let git_before = self.git_binding();
        let leaving = self.stash_local_state();
        let slot = self.documents.new_slot();
        self.documents.push_active(incoming, leaving, slot);
        self.adopt_local_state(SlotLocalState {
            path,
            ..Default::default()
        });
        self.finish_document_switch(git_before);
        true
    }

    /// Whether the on-screen document is a blank untitled one nobody touched,
    /// which a freshly opened file may take the place of.
    fn active_document_is_pristine_untitled(&self) -> bool {
        let state = self.host.editor_state();
        self.current_path.is_none()
            && !state.is_dirty()
            && state.document_revision() == 0
            && !state.history.can_undo()
    }

    /// Open the document at `path` in a new tab, or switch to it when it is
    /// already open. A blank untitled tab that nobody touched is replaced
    /// rather than left beside it. Returns whether the document is showing.
    pub(crate) fn open_document_tab(&mut self, path: PathBuf) -> bool {
        self.open_document_tab_inner(path, true)
    }

    fn open_document_tab_inner(&mut self, path: PathBuf, record_recent: bool) -> bool {
        self.sync_active_slot();
        if let Some(index) = self.documents.index_of_path(&path) {
            return self.activate_document_slot(index);
        }
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return false;
        }
        let replaces_blank = self.active_document_is_pristine_untitled();
        let loaded = match persistence::load_document(&mut self.host, &path, false) {
            Ok(Some(loaded)) => loaded,
            Ok(None) => return false,
            Err(error) => {
                eprintln!("[open] {error}");
                persistence::show_error_dialog_public(
                    &self.host,
                    ErrorKind::Open,
                    Some(&path),
                    &error.to_string(),
                );
                return false;
            }
        };
        let LoadedDocument {
            state,
            bound_path,
            thumbs,
            open_in_normal_mode,
        } = loaded;
        // A legacy-schema upgrade can bind the document to a different file.
        if let Some(index) = self.documents.index_of_path(&bound_path) {
            return self.activate_document_slot(index);
        }
        let blank_index = self.documents.active_index();
        let incoming = ParkedDocument::from_state(state, thumbs);
        if !self.install_new_document_tab(incoming, Some(bound_path.clone())) {
            return false;
        }
        self.host.editor_state_mut().editor_ui.home.hide();
        if open_in_normal_mode {
            persistence::open_loaded_normal_workspace(&mut self.host);
        }
        self.host.editor_state_mut().mark_saved_revision();
        self.host.arm_missing_fonts_detection();
        persistence::fit_loaded_document(&mut self.host, self.window.as_ref());
        if record_recent {
            crate::settings_io::touch_recent(&mut self.host, &bound_path);
        }
        if replaces_blank {
            self.documents.remove(blank_index);
            self.publish_document_tabs();
        }
        true
    }

    /// Cmd+N — a new untitled document in its own tab.
    pub(crate) fn open_untitled_tab(&mut self) -> bool {
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return false;
        }
        let incoming = ParkedDocument::from_state(EditorState::starter(), Default::default());
        if !self.install_new_document_tab(incoming, None) {
            return false;
        }
        self.host.editor_state_mut().editor_ui.home.hide();
        self.host
            .fit_content_to_viewport(self.viewport_width, self.viewport_height);
        self.host.mark_editor_state_dirty();
        true
    }

    /// Cmd+O — pick a file and open it in a tab.
    pub(crate) fn open_document_via_picker(&mut self) -> bool {
        self.host.commit_variable_row_focus_if_any_pub();
        match persistence::pick_open_path(&self.host) {
            Some(path) => self.open_document_tab(path),
            None => false,
        }
    }

    /// File ▸ Open Recent. A recent entry whose file is gone is pruned.
    pub(crate) fn open_recent_document(&mut self, index: usize) -> bool {
        let Some(entry) = self
            .host
            .editor_state()
            .editor_ui
            .recent_files
            .get(index)
            .cloned()
        else {
            return false;
        };
        let path = PathBuf::from(&entry.path);
        if self.open_document_tab(path.clone()) {
            return true;
        }
        if !path.is_file() {
            self.host
                .editor_state_mut()
                .editor_ui
                .recent_files
                .retain(|recent| recent.path != entry.path);
            self.host.mark_editor_state_dirty();
        }
        false
    }

    /// File-menu actions that open or create a document go to tabs; the rest
    /// run through `persistence::run_action` against the document on screen.
    pub(crate) fn run_file_action(&mut self, action: FileAction) -> ActionOutcome {
        match action {
            FileAction::New => {
                self.open_untitled_tab();
                ActionOutcome::Noop
            }
            FileAction::Open => {
                self.open_document_via_picker();
                ActionOutcome::Noop
            }
            FileAction::OpenRecent(index) => {
                self.open_recent_document(index);
                ActionOutcome::Noop
            }
            other => persistence::run_action(
                other,
                &mut self.host,
                &mut self.current_path,
                self.window.as_ref(),
            ),
        }
    }

    /// Close the tab at `index`, asking first when it has unsaved edits. The
    /// last tab never closes to nothing: it is replaced by a fresh untitled
    /// document. Returns whether the tab was closed.
    pub(crate) fn close_document_tab(&mut self, index: usize) -> bool {
        self.close_document_tab_with(index, |title, body| {
            crate::message_dialog::ask_yes_no_cancel(title, body, rfd::MessageLevel::Warning)
        })
    }

    /// [`Self::close_document_tab`] with the question asked through `ask`.
    pub(crate) fn close_document_tab_with(
        &mut self,
        index: usize,
        mut ask: impl FnMut(&str, &str) -> Option<crate::message_dialog::Choice>,
    ) -> bool {
        self.sync_active_slot();
        if index >= self.documents.len() {
            return false;
        }
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return false;
        }
        if self.documents.is_dirty(index, self.document_is_dirty()) {
            // Ask against the document itself so Save writes the right file.
            if !self.activate_document_slot(index) || !self.confirm_close_active_document(&mut ask)
            {
                return false;
            }
        }
        self.discard_document_tab(index)
    }

    /// Remove the tab at `index` without asking. When it is the one on screen,
    /// a neighbour (or, for the only tab, a fresh untitled document) takes over.
    pub(crate) fn discard_document_tab(&mut self, index: usize) -> bool {
        let Some(id) = self.documents.id_at(index) else {
            return false;
        };
        if index == self.documents.active_index() {
            if self.documents.len() == 1 {
                if !self.open_untitled_tab() {
                    return false;
                }
            } else {
                let neighbour = if index + 1 < self.documents.len() {
                    index + 1
                } else {
                    index - 1
                };
                if !self.activate_document_slot(neighbour) {
                    return false;
                }
            }
        }
        // Appending or switching moved positions; find the tab again by id.
        let Some(index) = self.documents.index_of(id) else {
            return false;
        };
        let removed = self.documents.remove(index).is_some();
        self.publish_document_tabs();
        removed
    }

    /// Reopen the documents saved at the last quit. Files that are gone are
    /// skipped; the document that was on screen comes back on screen.
    pub(crate) fn restore_session_documents(&mut self) {
        let entries = std::mem::take(&mut self.pending_session_restore);
        let mut active_path = None;
        for entry in entries {
            let path = PathBuf::from(&entry.path);
            if !path.is_file() {
                continue;
            }
            if self.open_document_tab_inner(path.clone(), false) && entry.active {
                active_path = Some(path);
            }
        }
        if let Some(path) = active_path {
            if let Some(index) = self.documents.index_of_path(&path) {
                self.activate_document_slot(index);
            }
        }
    }

    /// Deliver a save acknowledgement that no longer matches the document on
    /// screen to the parked document that issued it, so a save started in one
    /// tab marks THAT tab clean. Returns whether a parked document owned it.
    pub(crate) fn apply_parked_save_ack(
        &mut self,
        epoch: u64,
        generation: u64,
        revision: u64,
        bound_path: Option<&std::path::Path>,
    ) -> bool {
        let EpochOwner::Parked(index) = self
            .documents
            .owner_of_epoch(epoch, self.host.document_epoch())
        else {
            return false;
        };
        let Some(slot) = self.documents.slot_mut(index) else {
            return false;
        };
        if let Some(path) = bound_path {
            slot.local.path = Some(path.to_path_buf());
            slot.local.git_session = None;
        }
        let Some(parked) = slot.parked.as_mut() else {
            return false;
        };
        if let Some(path) = bound_path {
            parked.scope_mut().set_file_name_display(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
            );
        }
        let marked = parked
            .scope_mut()
            .mark_saved_revision_at(generation, revision);
        self.publish_document_tabs();
        marked
    }
}

#[cfg(test)]
#[path = "document_tabs_host_tests.rs"]
mod tests;
