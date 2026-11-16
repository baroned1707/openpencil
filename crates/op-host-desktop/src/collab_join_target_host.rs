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
//! is under way (Starting / Joining / Authenticating) the tab system refuses to
//! switch (`collab_blocks_document_switch`), so nothing can switch away from
//! the join's tab before the Snapshot lands. The cleanup below still judges the
//! tab by ITS slot (epoch and phase), on screen or parked, never by what is
//! showing. One session per process: a join (or start) is refused while another
//! document holds a session.
//!
//! The session's display state (queued join action, typed address, panel, a
//! failure notice) moves with the join: out of the guest's tab into the new
//! one, and back again if the join falls through, so the user sees the same
//! panel before, during and after.
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
        // One session per process: a document that holds one makes every start,
        // join or discovery from another document a refusal.
        if self.refuse_collab_action_while_another_document_collaborates() {
            return true;
        }
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
        // A tab that already holds a session cannot start another one in a new
        // tab: that would be two sessions in one process.
        if self.host.editor_state().editor_ui.collab.binds_session() {
            eprintln!("[collab] join refused, this document already holds a session");
            self.show_collab_toast(crate::collab_slots_host::ALREADY_ACTIVE_KEY);
            return false;
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
        // The queued join, the typed address and the open panel belong to the
        // session that is about to run, which lives in the NEW tab. Lift the
        // session out of the old tab for the swap and put it into whichever
        // tab is on screen afterwards (the old one again if no tab opened).
        let session = self.host.editor_state_mut().editor_ui.collab.take_session();
        let opened = self.open_untitled_tab_unchecked();
        self.host
            .editor_state_mut()
            .editor_ui
            .collab
            .restore_session(session);
        if !opened {
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
        // Judge the join's own tab, wherever it is: its epoch and its session
        // phase, from the slot that holds it.
        let on_screen = self.documents.active_id() == tab.document;
        let Some((epoch, phase)) = self.join_tab_epoch_and_phase(tab.document, on_screen) else {
            self.guest_join_tab = None;
            return false;
        };
        if epoch != tab.epoch {
            // The Snapshot installed: the tab is the shared document and stays.
            self.guest_join_tab = None;
            return false;
        }
        // Still being connected or admitted: nothing to settle yet.
        if phase.binds_session() {
            return false;
        }
        self.guest_join_tab = None;
        if !on_screen {
            return self.close_unused_parked_join_tab(tab.document);
        }
        // Never close anything that has content.
        if !self.active_document_is_pristine_untitled() {
            return false;
        }
        let Some(previous) = self.documents.index_of(tab.previous) else {
            return false;
        };
        // The session UI (a failure notice, the panel) must outlive the closed
        // tab: it moves back to the guest's own tab, which is where the user
        // lands and where they started the join.
        let session = self.host.editor_state_mut().editor_ui.collab.take_session();
        let activated = self.activate_document_slot(previous);
        self.host
            .editor_state_mut()
            .editor_ui
            .collab
            .restore_session(session);
        if !activated {
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

    /// The epoch and session phase of the document `id`, on screen or parked.
    /// `None` when no such tab is open any more.
    fn join_tab_epoch_and_phase(
        &self,
        id: DocumentId,
        on_screen: bool,
    ) -> Option<(u64, CollabConnectionPhase)> {
        if on_screen {
            return Some((
                self.host.document_epoch(),
                self.host.editor_state().editor_ui.collab.phase,
            ));
        }
        let index = self.documents.index_of(id)?;
        let parked = self.documents.parked_document(index)?;
        Some((parked.epoch(), parked.scope().collab().phase))
    }

    /// A join that fell through while the user was looking at another tab: the
    /// empty tab it opened is closed, unless something was put in it.
    fn close_unused_parked_join_tab(&mut self, id: DocumentId) -> bool {
        let Some(index) = self.documents.index_of(id) else {
            return false;
        };
        let pristine = self
            .documents
            .parked_document(index)
            .is_some_and(|parked| parked.scope().is_pristine());
        if !pristine || !self.remove_slot(index) {
            return false;
        }
        self.recovery_forget(id);
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

#[cfg(test)]
#[path = "collab_join_handoff_tests.rs"]
mod handoff_tests;
