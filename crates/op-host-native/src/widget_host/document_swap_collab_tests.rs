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

    // The parked document has a session bound (Ended), so only the session's
    // own pump entry may run against it.
    let seen_inside = host
        .with_parked_collab_session(&mut parked, |host| {
            let collab = &host.editor_state().editor_ui.collab;
            (collab.phase, collab.availability)
        })
        .expect("a document with a session bound is reachable through the pump lens");

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

fn session_in_phase(phase: CollabConnectionPhase) -> op_editor_core::EditorState {
    let mut state = op_editor_core::EditorState::starter();
    state.editor_ui.collab.set_authenticated_session(
        phase,
        AuthenticatedCollabSession {
            session_name: "Review".into(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        Vec::new(),
    );
    state
}

#[test]
fn switching_tabs_is_not_a_replacement_so_a_live_session_does_not_refuse_it() {
    for phase in [
        CollabConnectionPhase::Active,
        CollabConnectionPhase::Reconnecting,
        CollabConnectionPhase::ReadOnly,
    ] {
        let mut host = host_with_app_prefs();
        host.editor_state_mut()
            .editor_ui
            .collab
            .set_authenticated_session(
                phase,
                AuthenticatedCollabSession {
                    session_name: "Review".into(),
                    role: CollabUiRole::Editor,
                    share_endpoint: None,
                },
                Vec::new(),
            );
        let mut parked =
            ParkedDocument::from_state(op_editor_core::EditorState::starter(), Default::default());

        assert!(host.swap_document(&mut parked), "{phase:?}");

        // The session left with its document and is still intact there.
        assert_eq!(parked.scope().collab().phase, phase);
        assert_eq!(
            host.editor_state().editor_ui.collab.phase,
            CollabConnectionPhase::Idle
        );
        assert!(parked.collab_session_is_bound());
    }
}

#[test]
fn the_collab_pump_lens_reaches_only_a_document_with_a_session() {
    let mut host = host_with_app_prefs();
    let mut plain =
        ParkedDocument::from_state(op_editor_core::EditorState::starter(), Default::default());
    let mut ran = false;
    assert!(host
        .with_parked_collab_session(&mut plain, |_host| ran = true)
        .is_none());
    assert!(
        !ran,
        "the narrow entry is not a way around the general lens"
    );

    let mut shared = ParkedDocument::from_state(
        session_in_phase(CollabConnectionPhase::Active),
        Default::default(),
    );
    let seen = host.with_parked_collab_session(&mut shared, |host| {
        // What an applied remote change does to the host.
        op_collab_host::CollabHost::mark_editor_state_dirty(host);
        host.editor_state().editor_ui.collab.phase
    });
    assert_eq!(seen, Some(CollabConnectionPhase::Active));
    assert_eq!(
        host.editor_state().editor_ui.collab.phase,
        CollabConnectionPhase::Idle,
        "the screen is restored"
    );
}

#[test]
fn a_read_only_look_is_allowed_for_a_shared_document() {
    let mut host = host_with_app_prefs();
    let mut shared = ParkedDocument::from_state(
        session_in_phase(CollabConnectionPhase::Active),
        Default::default(),
    );
    let phase = host.read_parked_document(&mut shared, |host| {
        host.editor_state().editor_ui.collab.phase
    });
    assert_eq!(phase, CollabConnectionPhase::Active);
    assert_eq!(
        host.editor_state().editor_ui.collab.phase,
        CollabConnectionPhase::Idle
    );
    assert_eq!(shared.scope().collab().phase, CollabConnectionPhase::Active);
}
