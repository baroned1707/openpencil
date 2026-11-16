//! Fixtures for the tests of collaboration sessions across open documents.
//!
//! The open-source build has no ticket issuer, so no real session can start.
//! `op_collab_host::test_support` stands up the runtimes (an owner with a peer, a
//! guest with a remote commit waiting); these helpers give the document that
//! owns one the session display a real session would have shown.

use crate::DesktopApp;
use jian_ops_schema::PenDocument;
use op_editor_core::{
    AuthenticatedCollabSession, CollabConnectionPhase, CollabParticipantUi, CollabUiRole,
    CollabUiState,
};

/// Show `phase` with a session of `role` and `others` other participants.
pub(crate) fn give_session(
    collab: &mut CollabUiState,
    phase: CollabConnectionPhase,
    role: CollabUiRole,
    others: usize,
) {
    let mut participants = vec![CollabParticipantUi::new(
        "self", "Me", 0x112233ff, role, true,
    )];
    for index in 0..others {
        participants.push(CollabParticipantUi::new(
            format!("peer-{index}"),
            format!("Peer {index}"),
            0x445566ff,
            CollabUiRole::Editor,
            false,
        ));
    }
    collab.set_authenticated_session(
        phase,
        AuthenticatedCollabSession {
            session_name: "Review".into(),
            role,
            share_endpoint: None,
        },
        participants,
    );
}

/// `document` with its first node renamed: the smallest change a remote peer
/// can commit that a test can see.
pub(crate) fn renamed(document: &PenDocument, name: &str) -> PenDocument {
    use op_editor_core::PenNodeExt;
    let mut document = document.clone();
    document.children[0].base_mut().name = Some(name.to_string());
    document
}

/// The name of the first node of the PARKED document at `index`.
pub(crate) fn parked_first_node_name(app: &DesktopApp, index: usize) -> Option<String> {
    use op_editor_core::PenNodeExt;
    app.documents
        .parked_document(index)?
        .scope()
        .doc()
        .children
        .first()?
        .base()
        .name
        .clone()
}

/// The session phase of the PARKED document at `index`.
pub(crate) fn parked_phase(app: &DesktopApp, index: usize) -> CollabConnectionPhase {
    app.documents
        .parked_collab(index)
        .expect("a parked document")
        .phase
}

/// Everything about the document on screen a poll of another tab must leave
/// alone.
pub(crate) fn on_screen_fingerprint(app: &DesktopApp) -> String {
    format!(
        "{:?}|epoch={}",
        app.host.editor_state(),
        app.host.document_epoch()
    )
}

/// An app whose first tab holds `runtime`'s session (shown as `phase`, as
/// `role`, with `others` other people), with a second, plain tab opened on top:
/// the session's document is parked at index 0, its runtime stashed in the
/// slot, and the plain tab is on screen.
pub(crate) fn app_with_parked_session(
    runtime: crate::collab_runtime::DesktopCollabRuntime,
    phase: CollabConnectionPhase,
    role: CollabUiRole,
    others: usize,
) -> DesktopApp {
    let mut app = DesktopApp::new(None);
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        phase,
        role,
        others,
    );
    assert!(app.open_untitled_tab(), "a live session does not block it");
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents.active_index(), 1);
    app
}
