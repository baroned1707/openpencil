//! Which part of [`CollabUiState`] belongs to one open document.
//!
//! The display state mixes two kinds of data. SESSION data describes the
//! collaboration session bound to a document: its phase, participants,
//! presence, notices, queued action, and the panel the user is looking at for
//! it. It travels with the document, so a session shown in one tab never shows
//! in another. APP data describes the running app: whether collaboration is
//! available at all, which transports this build offers, and the preferred
//! relay region. It never leaves the live state, so whichever document is on
//! screen always reads the app's current values.
//!
//! [`CollabUiState::exchange_session`] swaps the session half and leaves the
//! app half exactly where it is. It destructures [`CollabUiState`] and
//! [`CollabPanelState`] on both sides with no `..` rest pattern: a new field
//! does not compile until it is classified as session (swap it) or app
//! (`name: _`). A field that is neither clearly one nor the other is treated as
//! app-level, the safer default: nothing about it can then leak between
//! documents or be lost on a switch.

use std::mem::swap;

use super::{CollabPanelState, CollabUiState};

impl CollabUiState {
    /// Swap every SESSION field with `other`. APP fields of both stay.
    pub fn exchange_session(&mut self, other: &mut CollabUiState) {
        let CollabUiState {
            // App-level: owned by the host runtime, rewritten every frame.
            availability: _,
            transport_capabilities: _,
            phase,
            panel,
            pending_edit,
            notice,
            discarded_edit,
            pending_action,
            authenticated,
            public_session,
            pending_admissions,
            pending_owner_confirmation,
            participants,
            presence,
            queued_presence,
            last_presence_flush_ms,
        } = self;
        let CollabUiState {
            availability: _,
            transport_capabilities: _,
            phase: other_phase,
            panel: other_panel,
            pending_edit: other_pending_edit,
            notice: other_notice,
            discarded_edit: other_discarded_edit,
            pending_action: other_pending_action,
            authenticated: other_authenticated,
            public_session: other_public_session,
            pending_admissions: other_pending_admissions,
            pending_owner_confirmation: other_pending_owner_confirmation,
            participants: other_participants,
            presence: other_presence,
            queued_presence: other_queued_presence,
            last_presence_flush_ms: other_last_presence_flush_ms,
        } = other;
        swap(phase, other_phase);
        exchange_panel_session(panel, other_panel);
        swap(pending_edit, other_pending_edit);
        swap(notice, other_notice);
        swap(discarded_edit, other_discarded_edit);
        swap(pending_action, other_pending_action);
        swap(authenticated, other_authenticated);
        swap(public_session, other_public_session);
        swap(pending_admissions, other_pending_admissions);
        swap(pending_owner_confirmation, other_pending_owner_confirmation);
        swap(participants, other_participants);
        swap(presence, other_presence);
        swap(queued_presence, other_queued_presence);
        swap(last_presence_flush_ms, other_last_presence_flush_ms);
    }

    /// Lift the session out, leaving an idle one behind. The returned value
    /// carries meaningful SESSION fields only; hand it back with
    /// [`Self::restore_session`], on this state or on the document that is on
    /// screen after a tab change.
    pub fn take_session(&mut self) -> CollabUiState {
        let mut taken = CollabUiState::default();
        self.exchange_session(&mut taken);
        taken
    }

    /// Install a session lifted by [`Self::take_session`]; the session being
    /// replaced is dropped.
    pub fn restore_session(&mut self, mut session: CollabUiState) {
        self.exchange_session(&mut session);
    }
}

/// The panel's per-session half. `relay_region` is the preferred hub and is
/// app-level.
fn exchange_panel_session(panel: &mut CollabPanelState, other: &mut CollabPanelState) {
    let CollabPanelState {
        open,
        view,
        join_input,
        join_address_focused,
        hover,
        discovered,
        relay_region: _,
    } = panel;
    let CollabPanelState {
        open: other_open,
        view: other_view,
        join_input: other_join_input,
        join_address_focused: other_join_address_focused,
        hover: other_hover,
        discovered: other_discovered,
        relay_region: _,
    } = other;
    swap(open, other_open);
    swap(view, other_view);
    swap(join_input, other_join_input);
    swap(join_address_focused, other_join_address_focused);
    swap(hover, other_hover);
    swap(discovered, other_discovered);
}

#[cfg(test)]
pub(crate) mod tests;
