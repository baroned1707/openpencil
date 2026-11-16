//! Collaboration sessions across open documents.
//!
//! A session stays attached to ONE document while the user freely switches
//! tabs. The runtime that carries it (sockets, actor, ticket renewal) lives
//! with that document:
//!
//! - `DesktopApp::collab_runtime` is always the runtime of the document ON
//!   SCREEN, so every call site that drives "the" runtime stays correct.
//! - A document whose session is bound keeps its runtime in its registry slot
//!   (`SlotLocalState::collab_runtime`) while it is parked. A document without
//!   a session shares ONE runtime that follows the screen: discovery results,
//!   the queued join target and the cached region preference are not tied to a
//!   tab, exactly as before.
//! - Every frame (and every wake) each parked session is polled through the
//!   host's lens, so remote commits keep landing, tickets keep renewing and the
//!   bounded event lanes never back up. That is invisible: no redraw unless the
//!   session's phase changed, and the document on screen is not touched.
//!
//! Still ONE session per process: starting or joining while another document
//! holds one is refused (`documents.collabAlreadyActive`).

use crate::collab_runtime;
use crate::document_registry::SlotLocalState;
use crate::message_dialog::Choice;
use crate::DesktopApp;
use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{CollabConnectionPhase, CollabUiAction, CollabUiRole};
use op_host_native::widget_host::ParkedDocument;

/// Locale key of the notice shown when a session cannot start or be joined
/// because another document already holds one.
pub(crate) const ALREADY_ACTIVE_KEY: &str = "documents.collabAlreadyActive";
const END_TITLE_KEY: &str = "dialog.collabEndTitle";
const OWNER_END_BODY_KEY: &str = "dialog.collabOwnerEndBody";
const GUEST_LEAVE_BODY_KEY: &str = "dialog.collabGuestLeaveBody";

/// What closing a document costs the person who closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionStake {
    /// The document is this person's own, shared with `others` participants:
    /// closing it ends the session for them.
    Owner { others: usize },
    /// The document is someone else's, held as a guest: closing it leaves.
    Guest,
}

impl DesktopApp {
    // ---- switching -------------------------------------------------------

    /// Whether the session of the document on screen forbids switching away
    /// right now. Only a session that is not established yet (the transport or
    /// admission is still being brought up) or a local edit that is still being
    /// captured blocks it; a live session, a reconnecting guest, a read-only or
    /// ended one all stay switchable.
    pub(crate) fn collab_blocks_document_switch(&self) -> bool {
        self.collab_runtime.local_edit_in_flight()
            || self
                .host
                .editor_state()
                .editor_ui
                .collab
                .phase
                .is_being_established()
    }

    /// Run just before the document on screen is parked: commit half-typed
    /// input INSIDE an edit capture, so it reaches the other participants
    /// rather than sitting in a document nobody is looking at, and tell the
    /// peers this cursor is gone.
    pub(crate) fn prepare_collab_for_leaving_document(&mut self) {
        let collab = &self.host.editor_state().editor_ui.collab;
        if !collab.binds_session() {
            return;
        }
        let can_edit = collab.phase == CollabConnectionPhase::Active
            && collab.authenticated_session().is_some_and(|session| {
                matches!(session.role, CollabUiRole::Owner | CollabUiRole::Editor)
            });
        // Same shape as every GUI gesture (`on_ime_commit`): open the capture,
        // do the edit, close it. A capture that cannot open (a read-only role,
        // no live actor) leaves nothing to commit that could reach peers.
        let capture = can_edit && self.collab_runtime.begin_local_edit(&mut self.host);
        self.host.commit_pending_input_for_switch();
        if capture {
            self.collab_runtime.finish_local_edit(&mut self.host);
        }
        self.collab_runtime
            .publish_local_presence_immediately(&mut self.host, None);
    }

    /// Decide which runtime serves the document that is about to be on screen.
    /// Called right after the host swapped documents, with the registry state
    /// of both sides in hand: `leaving_doc` is the document that just left the
    /// screen.
    ///
    /// | leaving has a session | arriving owns a runtime | result |
    /// |---|---|---|
    /// | yes | yes | leaving keeps the runtime in its slot; arriving's is adopted |
    /// | yes | no  | leaving keeps it; a fresh idle runtime serves the screen |
    /// | no  | yes | the shared runtime is dropped; arriving's is adopted |
    /// | no  | no  | the shared runtime follows the screen |
    pub(crate) fn rotate_collab_runtimes(
        &mut self,
        leaving_doc: &mut ParkedDocument,
        leaving: &mut SlotLocalState,
        arriving: &mut SlotLocalState,
    ) {
        let leaving_has_session = leaving_doc.collab_session_is_bound();
        match (leaving_has_session, arriving.collab_runtime.take()) {
            (true, Some(own)) => {
                leaving.collab_runtime =
                    Some(Box::new(std::mem::replace(&mut self.collab_runtime, *own)));
            }
            (true, None) => {
                leaving.collab_runtime = Some(Box::new(std::mem::replace(
                    &mut self.collab_runtime,
                    collab_runtime::new_runtime(),
                )));
            }
            (false, Some(own)) => {
                // The shared runtime is idle or discovering. Discovery was
                // started in the document that is leaving; it cannot follow the
                // user onto a document with a session of its own, so it ends
                // there (the runtime's worker stops when it is dropped).
                let leaving_collab = leaving_doc.scope_mut().collab_mut();
                if leaving_collab.phase == CollabConnectionPhase::Discovering {
                    leaving_collab.set_phase(CollabConnectionPhase::Idle);
                    leaving_collab.panel.discovered = Default::default();
                }
                self.collab_runtime = *own;
            }
            (false, None) => return,
        }
        // The relay region is one app-wide preference, but every runtime caches
        // it: hand the arriving runtime the value the panel shows.
        let region = self.host.editor_state().editor_ui.collab.panel.relay_region;
        self.collab_runtime.adopt_relay_region(region);
    }

    // ---- polling ---------------------------------------------------------

    /// Poll every collaboration runtime: the one serving the screen, then the
    /// runtime of each parked document that has a session. Returns whether the
    /// screen needs a repaint. A parked session asks for one only when its
    /// phase changed; its remote commits are applied to ITS document and are
    /// not visible.
    pub(crate) fn poll_collaboration(&mut self) -> bool {
        let mut changed = self.collab_runtime.poll(&mut self.host);
        for status in self.collab_runtime.drain_status_events() {
            eprintln!("[collab] {status:?}");
        }
        for index in self.documents.parked_with_collab_session() {
            changed |= self.poll_parked_collab_session(index);
        }
        changed
    }

    /// The soonest moment any collaboration runtime, on screen or parked, wants
    /// the event loop awake (a guest's reconnect deadline).
    pub(crate) fn next_collab_deadline(&self) -> Option<std::time::Instant> {
        [
            self.collab_runtime.next_reconnect_deadline(),
            self.documents.next_parked_collab_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Poll the session of the parked document at `index` through the lens.
    /// `true` when its phase changed.
    fn poll_parked_collab_session(&mut self, index: usize) -> bool {
        let Some((mut parked, mut runtime)) = self.documents.take_collab_session(index) else {
            return false;
        };
        let phase_changed = self.host.with_parked_collab_session(&mut parked, |host| {
            let before = host.editor_state().editor_ui.collab.phase;
            runtime.poll(host);
            host.editor_state().editor_ui.collab.phase != before
        });
        for status in runtime.drain_status_events() {
            eprintln!("[collab] {status:?}");
        }
        self.documents.return_collab_session(index, parked, runtime);
        phase_changed.unwrap_or(false)
    }

    // ---- one session per process ----------------------------------------

    /// Whether a document OTHER than the one on screen holds a session.
    pub(crate) fn another_document_has_a_session(&self) -> bool {
        !self.documents.parked_with_collab_session().is_empty()
    }

    /// Refuse a start / join / discovery request while a session is live in
    /// another document, and drop a Cancel that would only rotate the
    /// process-wide avatar epoch of that session. `true` when the queued
    /// action was handled here and must not reach the runtime.
    pub(crate) fn refuse_collab_action_while_another_document_collaborates(&mut self) -> bool {
        if !self.another_document_has_a_session() {
            return false;
        }
        let here_has_session = self.host.editor_state().editor_ui.collab.binds_session();
        let action = self
            .host
            .editor_state()
            .editor_ui
            .collab
            .pending_action
            .as_ref();
        let starts_session = matches!(
            action,
            Some(
                CollabUiAction::Start
                    | CollabUiAction::StartLan
                    | CollabUiAction::BeginDiscovery
                    | CollabUiAction::JoinDiscovered { .. }
                    | CollabUiAction::JoinAddress { .. }
            )
        );
        // A runtime with nothing to stop still advances the avatar epoch when
        // asked to leave, which would blank the live session's avatars.
        let idle_leave = !here_has_session
            && matches!(
                action,
                Some(
                    CollabUiAction::Cancel | CollabUiAction::Leave | CollabUiAction::DiscardPending
                )
            );
        if idle_leave {
            self.drop_queued_collab_action();
            return true;
        }
        if !starts_session {
            return false;
        }
        eprintln!("[collab] request refused, another document holds a session");
        self.show_collab_toast(ALREADY_ACTIVE_KEY);
        self.drop_queued_collab_action();
        true
    }

    fn drop_queued_collab_action(&mut self) {
        let _ = self
            .host
            .editor_state_mut()
            .editor_ui
            .collab
            .take_pending_action();
        self.host.mark_editor_state_dirty();
    }

    pub(crate) fn show_collab_toast(&mut self, key: &'static str) {
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            key,
            Vec::new(),
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }

    // ---- closing and quitting -------------------------------------------

    /// What the session of the document at `index` would cost its user to end,
    /// or `None` when the document has no session.
    pub(crate) fn slot_session_stake(&self, index: usize) -> Option<SessionStake> {
        let collab = if index == self.documents.active_index() {
            &self.host.editor_state().editor_ui.collab
        } else {
            self.documents.parked_collab(index)?
        };
        if !collab.binds_session() {
            return None;
        }
        Some(if collab.is_session_owner() {
            SessionStake::Owner {
                others: collab.other_participant_count(),
            }
        } else {
            SessionStake::Guest
        })
    }

    /// The position of the document that holds the session, if any.
    fn session_slot(&self) -> Option<usize> {
        (0..self.documents.len()).find(|&index| self.slot_session_stake(index).is_some())
    }

    fn session_question(&mut self, index: usize, stake: SessionStake) -> (String, String) {
        let locale = self.host.editor_state().editor_ui.locale;
        let name = self.slot_display_name(index);
        let body = match stake {
            SessionStake::Owner { others } => op_i18n::translate(locale, OWNER_END_BODY_KEY)
                .replace("{{name}}", &name)
                .replace("{{count}}", &others.to_string()),
            SessionStake::Guest => {
                op_i18n::translate(locale, GUEST_LEAVE_BODY_KEY).replace("{{name}}", &name)
            }
        };
        (op_i18n::translate(locale, END_TITLE_KEY).to_string(), body)
    }

    /// Closing a tab whose session is live asks first: an owner is told how
    /// many people it ends the session for, a guest is asked whether to leave.
    /// Only Yes goes ahead, and a missing dialog backend keeps the tab. Nothing
    /// is left here; the caller leaves once the close is certain.
    pub(crate) fn confirm_leave_session_to_close(
        &mut self,
        index: usize,
        ask: &mut impl FnMut(&str, &str) -> Option<Choice>,
    ) -> bool {
        let Some(stake) = self.slot_session_stake(index) else {
            return true;
        };
        let (title, body) = self.session_question(index, stake);
        ask(&title, &body) == Some(Choice::Yes)
    }

    /// Quit warns ONCE, before any save prompt, when a session is live. Yes goes
    /// on; No and Cancel abort the quit. Never blocks: a missing dialog backend
    /// lets the quit through (#197).
    pub(crate) fn confirm_leave_session_to_quit(
        &mut self,
        ask: &mut impl FnMut(&str, &str) -> Option<Choice>,
    ) -> bool {
        self.sync_active_slot();
        let Some(index) = self.session_slot() else {
            return true;
        };
        let Some(stake) = self.slot_session_stake(index) else {
            return true;
        };
        let (title, body) = self.session_question(index, stake);
        !matches!(ask(&title, &body), Some(Choice::No | Choice::Cancel))
    }

    /// End the session of the document at `index`: the on-screen runtime
    /// directly, a parked one through the lens. With `drop_if_unreachable` a
    /// runtime the lens will not run is dropped instead, which still asks its
    /// worker to say goodbye (the process is about to end).
    pub(crate) fn leave_slot_session(&mut self, index: usize, drop_if_unreachable: bool) {
        // Leaving advances the process-wide avatar epoch; a test binary guards
        // that registry (see `save_session`), production builds compile this out.
        #[cfg(test)]
        let _avatar_guard = crate::collab_avatar_host::lock_avatar_test_registry();
        if index == self.documents.active_index() {
            self.collab_runtime.leave(&mut self.host);
            return;
        }
        let Some((mut parked, mut runtime)) = self.documents.take_collab_session(index) else {
            return;
        };
        let left = self
            .host
            .with_parked_collab_session(&mut parked, |host| runtime.leave(host));
        if left.is_none() && drop_if_unreachable {
            self.documents.restore_parked(index, parked);
            drop(runtime);
            return;
        }
        self.documents.return_collab_session(index, parked, runtime);
    }

    /// Process exit: say goodbye for every session, parked ones included. The
    /// on-screen runtime leaves unconditionally (as it always did); a parked
    /// one leaves through the lens, falling back to being dropped.
    pub(crate) fn leave_every_session_on_exit(&mut self) {
        #[cfg(test)]
        let _avatar_guard = crate::collab_avatar_host::lock_avatar_test_registry();
        self.collab_runtime.leave(&mut self.host);
        for index in self.documents.parked_with_collab_session() {
            self.leave_slot_session(index, true);
        }
    }
}

#[cfg(test)]
#[path = "collab_slots_test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "collab_slots_switch_tests.rs"]
mod switch_tests;

#[cfg(test)]
#[path = "collab_slots_session_tests.rs"]
mod session_tests;

#[cfg(test)]
#[path = "collab_slots_lens_tests.rs"]
mod lens_tests;
