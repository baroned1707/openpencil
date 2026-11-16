//! A collaboration session belongs to its document; the app's collaboration
//! availability does not.

use super::*;
use crate::collab_ui_state::session_scope::tests::{assert_session_identical, populated_session};
use crate::{
    AuthenticatedCollabSession, CollabAvailability, CollabConnectionPhase, CollabNoticeKind,
    CollabParticipantUi, CollabRelayRegion, CollabTransportCapabilities, CollabUiRole,
};

/// A state whose collaboration display holds `session`, with the app-level
/// fields set to recognisable values.
fn with_session(session: crate::CollabUiState) -> EditorState {
    let mut state = EditorState::starter();
    state.editor_ui.collab = session;
    set_app_prefs(&mut state);
    state
}

fn set_app_prefs(state: &mut EditorState) {
    let collab = &mut state.editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.transport_capabilities = CollabTransportCapabilities::ALL;
    collab.panel.relay_region = CollabRelayRegion::Global;
}

fn assert_app_prefs(state: &EditorState) {
    let collab = &state.editor_ui.collab;
    assert_eq!(collab.availability, CollabAvailability::Ready);
    assert_eq!(
        collab.transport_capabilities,
        CollabTransportCapabilities::ALL
    );
    assert_eq!(collab.panel.relay_region, CollabRelayRegion::Global);
}

fn ended_session() -> crate::CollabUiState {
    let mut collab = crate::CollabUiState::default();
    collab.set_authenticated_session(
        CollabConnectionPhase::Ended,
        AuthenticatedCollabSession {
            session_name: "Finished review".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        vec![CollabParticipantUi::new(
            "p9",
            "Grace Hopper",
            0x445566ff,
            CollabUiRole::Owner,
            false,
        )],
    );
    collab.set_notice(CollabNoticeKind::OwnerLeft, 3);
    collab
}

#[test]
fn collab_session_state_travels_with_its_document() {
    let mut live = with_session(populated_session());
    let before = live.editor_ui.collab.clone();
    let mut parked = DocumentScope::from_state(EditorState::starter());

    live.switch_document_scope(&mut parked);
    // The arriving document has no session; the old one is parked intact.
    assert_eq!(live.editor_ui.collab.phase, CollabConnectionPhase::Idle);
    assert!(live.editor_ui.collab.participants().is_empty());
    assert!(live.editor_ui.collab.notice.is_none());
    assert_session_identical(&parked.editor_ui.collab, &before);

    live.switch_document_scope(&mut parked);
    assert_session_identical(&live.editor_ui.collab, &before);
}

#[test]
fn app_level_collab_prefs_follow_the_user_not_the_document() {
    let mut live = with_session(populated_session());
    // The arriving document was parked with different (stale) app values.
    let mut parked = DocumentScope::from_state(EditorState::starter());
    assert_eq!(
        parked.editor_ui.collab.availability,
        CollabAvailability::Unavailable
    );

    live.switch_document_scope(&mut parked);
    assert_app_prefs(&live);

    // A value the runtime publishes while B is showing is what A finds later.
    live.editor_ui.collab.availability = CollabAvailability::SignInRequired;
    live.switch_document_scope(&mut parked);
    assert_eq!(
        live.editor_ui.collab.availability,
        CollabAvailability::SignInRequired
    );
    assert_eq!(
        live.editor_ui.collab.panel.relay_region,
        CollabRelayRegion::Global
    );

    // The lens (pure exchange) leaves them alone too.
    live.exchange_document_scope(&mut parked);
    assert_eq!(
        live.editor_ui.collab.availability,
        CollabAvailability::SignInRequired
    );
    assert_eq!(
        live.editor_ui.collab.transport_capabilities,
        CollabTransportCapabilities::ALL
    );
}

#[test]
fn a_new_tab_starts_with_an_idle_session() {
    let mut live = with_session(populated_session());
    // A tab built from a brand-new state, as `open_untitled_tab` does.
    let mut incoming = DocumentScope::from_state(EditorState::starter());

    live.switch_document_scope(&mut incoming);

    let collab = &live.editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Idle);
    assert!(!collab.panel.open);
    assert!(collab.pending_action.is_none());
    assert!(collab.panel.join_input.text().is_empty());
    assert!(collab.authenticated_session().is_none());
    assert_eq!(collab.pending_edit, crate::CollabPendingEditUi::None);
    // ... while the app-level values are already the app's.
    assert_app_prefs(&live);
}

#[test]
fn ended_session_does_not_leak_into_another_document_after_a_swap() {
    let mut live = with_session(ended_session());
    let ended = live.editor_ui.collab.clone();
    let mut other = DocumentScope::from_state(EditorState::starter());

    live.switch_document_scope(&mut other);

    let collab = &live.editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Idle, "no stale Ended");
    assert!(collab.participants().is_empty(), "no stale roster");
    assert!(collab.notice.is_none(), "no stale notice");
    assert!(
        !collab.phase.freezes_document(),
        "the other document is not frozen by a finished session"
    );

    live.switch_document_scope(&mut other);
    assert_session_identical(&live.editor_ui.collab, &ended);
}

#[test]
fn lens_exchange_round_trips_collab_state_bit_for_bit() {
    let mut live = with_session(populated_session());
    let mut parked_state = EditorState::starter();
    parked_state.editor_ui.collab = ended_session();
    let live_before = live.editor_ui.collab.clone();
    let parked_before = parked_state.editor_ui.collab.clone();
    let mut parked = DocumentScope::from_state(parked_state);

    live.exchange_document_scope(&mut parked);
    // Inside the lens the parked document's own session is the one in view.
    assert_session_identical(&live.editor_ui.collab, &parked_before);
    live.exchange_document_scope(&mut parked);

    assert_session_identical(&live.editor_ui.collab, &live_before);
    assert_session_identical(&parked.editor_ui.collab, &parked_before);
    assert_app_prefs(&live);
}

#[test]
fn discovery_follows_the_user_across_a_switch_but_a_session_does_not() {
    let mut discovering = crate::CollabUiState::default();
    discovering.set_phase(CollabConnectionPhase::Discovering);
    discovering.panel.open = true;
    discovering.panel.view = crate::CollabPanelView::Join;
    let before = discovering.clone();
    let mut live = with_session(discovering);
    let mut other = DocumentScope::from_state(EditorState::starter());

    live.switch_document_scope(&mut other);

    // Discovery is not bound to a document: the runtime still publishes the
    // endpoint list to whatever is on screen.
    assert_session_identical(&live.editor_ui.collab, &before);
    assert_app_prefs(&live);
}

#[test]
fn discovery_stays_behind_when_the_arriving_document_has_a_session() {
    // The arriving document's own session display must reach the screen
    // intact; discovery does not overwrite it, and the arriving session does
    // not end up on the document that leaves.
    let mut discovering = crate::CollabUiState::default();
    discovering.set_phase(CollabConnectionPhase::Discovering);
    discovering.panel.open = true;
    let discovering_before = discovering.clone();
    let mut live = with_session(discovering);
    let session = populated_session();
    let session_before = session.clone();
    let mut other = DocumentScope::from_state(with_session(session));

    live.switch_document_scope(&mut other);

    assert_session_identical(&live.editor_ui.collab, &session_before);
    assert_session_identical(other.collab(), &discovering_before);
    assert_app_prefs(&live);
}
