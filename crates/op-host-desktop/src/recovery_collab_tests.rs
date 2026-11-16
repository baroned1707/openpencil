//! Collaboration session state is transient: it never reaches disk.

use super::test_support::*;
use op_editor_core::{
    AuthenticatedCollabSession, CollabConnectionPhase, CollabNoticeKind, CollabParticipantUi,
    CollabUiAction, CollabUiRole,
};

/// Distinctive text the session display holds; none of it may be written.
const SESSION_NAME: &str = "Quarterly roadmap review";
const PARTICIPANT_NAME: &str = "Grace Hopper";
const TYPED_ADDRESS: &str = "10.77.88.99:4321";

fn give_the_document_a_session(app: &mut crate::DesktopApp) {
    let collab = &mut app.host.editor_state_mut().editor_ui.collab;
    collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: SESSION_NAME.to_string(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        vec![CollabParticipantUi::new(
            "p1",
            PARTICIPANT_NAME,
            0x112233ff,
            CollabUiRole::Owner,
            false,
        )],
    );
    collab.set_notice(CollabNoticeKind::OwnerLeft, 1);
    collab.panel.open = true;
    collab.panel.join_input.set_text(TYPED_ADDRESS);
    collab.pending_action = Some(CollabUiAction::JoinAddress {
        endpoint: TYPED_ADDRESS.to_string(),
    });
}

#[test]
fn collab_state_is_not_persisted_in_recovery_snapshots() {
    let (mut app, _paths, work, recovery) = app_with_tabs("collab-state", &["a.op"]);
    edit_active(&mut app, "edit-under-a-session");
    give_the_document_a_session(&mut app);

    app.pump_recovery();
    app.flush_recovery();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(
        snapshot_name(&entries[0]).as_deref(),
        Some("edit-under-a-session")
    );
    for name in files_in(&recovery) {
        let Ok(bytes) = std::fs::read(recovery.join(&name)) else {
            continue; // a directory, not a file
        };
        let text = String::from_utf8_lossy(&bytes);
        for secret in [SESSION_NAME, PARTICIPANT_NAME, TYPED_ADDRESS, "joinAddress"] {
            assert!(!text.contains(secret), "{name} contains {secret:?}");
        }
    }
    // Reloading the snapshot yields an idle session.
    let (loaded, _) = op_host_native::widget_host::capture_loaded_thumbnails(|| {
        op_host_services::doc_io::load_editor_state(
            &entries[0].document_path,
            op_editor_core::Locale::EnUs,
        )
    });
    let collab = loaded.expect("snapshot loads").editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Idle);
    assert!(collab.participants().is_empty());
    assert!(collab.pending_action.is_none());
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

/// `editorMeta` (the only editor state written into the `.op`) has no
/// collaboration field, so a saved document cannot carry a session.
#[test]
fn editor_meta_carries_no_collab_state() {
    let mut state = op_editor_core::EditorState::starter();
    state.editor_ui.collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: SESSION_NAME.to_string(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        Vec::new(),
    );
    let meta = serde_json::to_string(&op_pen_loader::EditorMeta::from_state(&state))
        .expect("editorMeta serializes");
    assert!(!meta.to_lowercase().contains("collab"), "{meta}");
    assert!(!meta.contains(SESSION_NAME), "{meta}");
}
