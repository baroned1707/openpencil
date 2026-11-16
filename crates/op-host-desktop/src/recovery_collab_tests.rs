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

fn give_the_document_a_session(app: &mut crate::DesktopApp, role: CollabUiRole) {
    let collab = &mut app.host.editor_state_mut().editor_ui.collab;
    collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: SESSION_NAME.to_string(),
            role,
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
    // The owner's own document is persisted (a guest's never is, see below);
    // this test is about what of the session leaks into that snapshot.
    give_the_document_a_session(&mut app, CollabUiRole::Owner);

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

/// An unsaved edit in the document on screen, then a session that makes it
/// someone else's shared document.
fn become_a_guest(app: &mut crate::DesktopApp) {
    give_the_document_a_session(app, CollabUiRole::Editor);
}

fn snapshot_everything_due(app: &mut crate::DesktopApp) {
    app.pump_recovery();
    app.flush_recovery();
}

#[test]
fn guest_document_is_never_written_to_the_recovery_dir_on_screen_or_parked() {
    let (mut app, _paths, work, recovery) = app_with_tabs("guest-states", &["a.op", "b.op"]);
    edit_active(&mut app, "someone-elses-design");
    become_a_guest(&mut app);

    // On screen.
    snapshot_everything_due(&mut app);
    assert!(
        entries_in(&recovery).is_empty(),
        "on screen: {:?}",
        files_in(&recovery)
    );
    app.snapshot_dirty_documents(crate::recovery_store::SnapshotReason::Exit);
    app.flush_recovery();
    assert!(
        entries_in(&recovery).is_empty(),
        "on screen, at exit: {:?}",
        files_in(&recovery)
    );

    // Parked: switch to the other tab; the guest document is parked.
    assert!(app.activate_document_slot(0));
    snapshot_everything_due(&mut app);
    assert!(
        entries_in(&recovery).is_empty(),
        "parked: {:?}",
        files_in(&recovery)
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn joining_deletes_an_existing_recovery_entry_for_the_shared_document() {
    let (mut app, _paths, work, recovery) = app_with_tabs("guest-join", &["a.op"]);
    edit_active(&mut app, "before-the-join");
    snapshot_everything_due(&mut app);
    assert_eq!(entries_in(&recovery).len(), 1, "the edit was snapshotted");

    // The join starts: the document is now another person's shared document.
    app.host
        .editor_state_mut()
        .editor_ui
        .collab
        .set_phase(CollabConnectionPhase::Joining);
    snapshot_everything_due(&mut app);

    assert!(
        entries_in(&recovery).is_empty(),
        "the entry is deleted: {:?}",
        files_in(&recovery)
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn guests_own_earlier_tab_is_still_captured() {
    let (mut app, _paths, work, recovery) = app_with_tabs("guest-own", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "my-own-work");
    assert!(app.activate_document_slot(1));
    edit_active(&mut app, "their-design");
    become_a_guest(&mut app);

    snapshot_everything_due(&mut app);

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some("my-own-work"));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
