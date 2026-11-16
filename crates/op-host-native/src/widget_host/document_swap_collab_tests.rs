//! The collaboration session display moves with its document through the
//! host's two seams; the app-level collaboration values do not.

use super::*;
use op_editor_core::{
    AuthenticatedCollabSession, CollabAvailability, CollabConnectionPhase, CollabNoticeKind,
    CollabParticipantUi, CollabRelayRegion, CollabUiRole,
};

fn ended_session_state() -> op_editor_core::EditorState {
    let mut state = op_editor_core::EditorState::starter();
    state.editor_ui.collab.set_authenticated_session(
        CollabConnectionPhase::Ended,
        AuthenticatedCollabSession {
            session_name: "Finished review".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        vec![CollabParticipantUi::new(
            "p1",
            "Grace Hopper",
            0x112233ff,
            CollabUiRole::Owner,
            false,
        )],
    );
    state
        .editor_ui
        .collab
        .set_notice(CollabNoticeKind::OwnerLeft, 1);
    state
}

fn host_with_app_prefs() -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let collab = &mut host.editor_state_mut().editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.panel.relay_region = CollabRelayRegion::Global;
    host
}

#[test]
fn a_swap_does_not_leak_a_finished_session_and_keeps_the_app_prefs() {
    let mut host = host_with_app_prefs();
    // A parked document that still carries the remains of a finished session.
    let mut parked = ParkedDocument::from_state(ended_session_state(), Default::default());

    assert!(host.swap_document(&mut parked));
    let collab = &host.editor_state().editor_ui.collab;
    assert_eq!(
        collab.phase,
        CollabConnectionPhase::Ended,
        "its own session"
    );
    assert_eq!(collab.availability, CollabAvailability::Ready);
    assert_eq!(collab.panel.relay_region, CollabRelayRegion::Global);

    // The document that was showing parked an idle session, nothing stale.
    assert_eq!(parked.scope().collab().phase, CollabConnectionPhase::Idle);
    assert!(parked.scope().collab().notice.is_none());
}

#[test]
fn the_lens_shows_the_parked_documents_own_session_and_restores_the_screen() {
    let mut host = host_with_app_prefs();
    let mut parked = ParkedDocument::from_state(ended_session_state(), Default::default());

    let seen_inside = host
        .with_parked_document(&mut parked, |host| {
            let collab = &host.editor_state().editor_ui.collab;
            (collab.phase, collab.availability)
        })
        .expect("the on-screen document is idle, so the lens runs");

    assert_eq!(
        seen_inside,
        (CollabConnectionPhase::Ended, CollabAvailability::Ready)
    );
    let collab = &host.editor_state().editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Idle, "screen restored");
    assert_eq!(collab.availability, CollabAvailability::Ready);
    assert_eq!(parked.scope().collab().phase, CollabConnectionPhase::Ended);
    assert!(parked.scope().collab().notice.is_some());
}
