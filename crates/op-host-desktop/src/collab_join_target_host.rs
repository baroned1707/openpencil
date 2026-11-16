//! Where the shared document of a collaboration JOIN lands.
//!
//! A guest's session starts with the owner's authoritative Snapshot, which
//! replaces the document on screen. It may therefore only be installed into a
//! pristine untitled tab (`active_document_is_pristine_untitled`); anything
//! else gets a NEW untitled tab first, and the join runs into that, which
//! leaves the guest's own document untouched in its own tab.
//!
//! Ordering, on purpose: the tab is created and on screen BEFORE the runtime
//! is asked to start the join (`pump_collaboration_transition_with`), so the
//! Snapshot can never arrive while the old document is showing. Once a join
//! is under way the tab system is frozen (`requires_background_save_as_fork`),
//! so nothing can switch away from the join's tab before the Snapshot lands.
//!
//! Cleanup: a join that ends WITHOUT a Snapshot (cancelled, refused, failed)
//! closes the tab it opened and puts the guest's own document back on screen
//! (`settle_guest_join_tab`). Once the Snapshot has installed (the document
//! epoch rotated) the tab is the shared document and stays: an ended session
//! leaves it as an independent, untitled local fork that is saved through
//! Save As. Retry and automatic reconnects resume inside the join's own tab
//! and never open another.

use crate::DesktopApp;
use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{CollabConnectionPhase, CollabUiAction, DocumentId};

/// Locale key of the notice shown when a join is refused for lack of a tab.
const JOIN_BLOCKED_KEY: &str = "documents.joinBlocked";

impl DesktopApp {
    /// Run the queued collaboration action, if any. Called once per frame.
    /// Returns whether anything changed.
    pub(crate) fn pump_collaboration_transition(&mut self) -> bool {
        let edit_in_flight = self.collab_runtime.local_edit_in_flight();
        self.pump_collaboration_transition_with(edit_in_flight, |app| {
            app.collab_runtime.drain_ui_action(&mut app.host)
        })
    }

    /// [`Self::pump_collaboration_transition`] with the runtime step injected,
    /// so a test can stand in for the runtime (a real session needs a ticket
    /// issuer the open-source build does not have).
    pub(crate) fn pump_collaboration_transition_with(
        &mut self,
        edit_in_flight: bool,
        drain: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        let pending = self
            .host
            .editor_state()
            .editor_ui
            .collab
            .pending_action
            .as_ref();
        let starts_join = matches!(
            pending,
            Some(CollabUiAction::JoinDiscovered { .. } | CollabUiAction::JoinAddress { .. })
        );
        let transition_pending = starts_join
            || matches!(
                pending,
                Some(CollabUiAction::Start | CollabUiAction::StartLan | CollabUiAction::Retry)
            );
        // The barrier below CANCELS a running whole-file import. A join that
        // is going to be refused must not cost the user that import, so the
        // refusal comes first.
        if starts_join
            && !self.active_document_is_pristine_untitled()
            && (self.current_figma_import.is_some() || self.current_html_import.is_some())
        {
            eprintln!("[collab] join refused, an import is running");
            self.refuse_join();
            return self.drop_refused_join();
        }
        let transition_ready = if transition_pending {
            self.settle_document_io_before_collaboration();
            self.settle_git_before_collaboration_transition()
        } else {
            true
        };
        // A press that queues an action also opens the gesture's local-edit
        // capture, and the session's document actor refuses to activate a
        // peer (or run any other document transition) while that capture is
        // open. Draining here would turn every in-panel approval into a
        // spurious failure, so wait for the matching release — which finishes
        // the capture and requests another frame.
        if !transition_ready || edit_in_flight {
            return false;
        }
        if starts_join && !self.prepare_join_target() {
            return self.drop_refused_join();
        }
        drain(self)
    }

    /// Make the tab the shared document will install into the one on screen.
    /// `false` after telling the user why, with nothing changed.
    fn prepare_join_target(&mut self) -> bool {
        self.sync_active_slot();
        if self.active_document_is_pristine_untitled() {
            return true;
        }
        if let Some(reason) = self.document_switch_blocked() {
            eprintln!("[collab] join refused, no tab can be opened: {reason}");
            return self.refuse_join();
        }
        let previous = self.documents.active_id();
        // The original is about to be parked under a session that does not
        // lend parked documents to the recovery writer, so write its entry
        // now, while it is still on screen. It cannot change afterwards.
        self.snapshot_before_run();
        if !self.open_untitled_tab_unchecked() {
            return self.refuse_join();
        }
        self.guest_join_tab = Some(GuestJoinTab {
            document: self.documents.active_id(),
            previous,
            epoch: self.host.document_epoch(),
        });
        true
    }

    /// Drop the refused request so it does not repeat every frame.
    fn drop_refused_join(&mut self) -> bool {
        let _ = self
            .host
            .editor_state_mut()
            .editor_ui
            .collab
            .take_pending_action();
        self.host.mark_editor_state_dirty();
        true
    }

    fn refuse_join(&mut self) -> bool {
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            JOIN_BLOCKED_KEY,
            Vec::new(),
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
        false
    }

    /// Run once per frame after the runtime was polled: close the tab a join
    /// opened if the join is over and no Snapshot ever landed in it. Returns
    /// whether anything changed.
    pub(crate) fn settle_guest_join_tab(&mut self) -> bool {
        let Some(tab) = self.guest_join_tab else {
            return false;
        };
        let on_screen = self.documents.active_id() == tab.document;
        let received_snapshot = self.host.document_epoch() != tab.epoch;
        if !on_screen || received_snapshot {
            self.guest_join_tab = None;
            return false;
        }
        let join_over = matches!(
            self.host.editor_state().editor_ui.collab.phase,
            CollabConnectionPhase::Idle | CollabConnectionPhase::Discovering
        );
        if !join_over {
            return false;
        }
        self.guest_join_tab = None;
        // Never close anything that has content.
        if !self.active_document_is_pristine_untitled() {
            return false;
        }
        let Some(previous) = self.documents.index_of(tab.previous) else {
            return false;
        };
        if !self.activate_document_slot(previous) {
            return false;
        }
        if let Some(index) = self.documents.index_of(tab.document) {
            if self.remove_slot(index) {
                self.recovery_forget(tab.document);
            }
        }
        self.publish_document_tabs();
        true
    }
}

/// The empty tab a join opened for the shared document.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GuestJoinTab {
    pub(crate) document: DocumentId,
    /// The guest's own document, put back on screen if the join falls through.
    pub(crate) previous: DocumentId,
    /// Document epoch the tab started with. An installed Snapshot rotates it.
    pub(crate) epoch: u64,
}

#[cfg(test)]
#[path = "collab_join_target_host_tests.rs"]
mod tests;
