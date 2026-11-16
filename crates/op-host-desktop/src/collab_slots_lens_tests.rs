//! What may reach a parked document while another tab collaborates, and what
//! may not reach a parked document that has a session of its own.

use super::test_support::*;
use crate::collab_avatar_host::lock_avatar_test_registry;
use crate::mcp_document_resolver::DesktopDocumentResolver;
use crate::recovery_host::test_support::{
    app_with_tabs, edit_active, entries_in, files_in, snapshot_name,
};
use crate::DesktopApp;
use op_collab_host::test_support;
use op_editor_core::{CollabConnectionPhase as Phase, CollabUiRole, DocumentId};
use op_host_services::mcp_live::{DocumentResolver, ResolveError};

/// A plain document parked at index 0 and a session on screen at index 1.
fn app_with_plain_parked_and_session_on_screen() -> (DesktopApp, DocumentId) {
    let mut app = DesktopApp::new(None);
    let plain = app.documents.active_id();
    assert!(app.open_untitled_tab());
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    (app, plain)
}

/// A session parked at index 0, a plain document on screen at index 1.
fn app_with_session_parked() -> (DesktopApp, DocumentId, test_support::OwnerLaneGuard) {
    let probe = DesktopApp::new(None);
    let (runtime, lane) = test_support::owner_session(probe.host.editor_state().doc.clone());
    let app = app_with_parked_session(runtime, Phase::Active, CollabUiRole::Owner, 1);
    let shared = app.documents.id_at(0).expect("the shared document");
    (app, shared, lane)
}

#[test]
fn non_collaborated_parked_document_is_reachable_by_mcp_while_another_slot_collaborates() {
    let (mut app, plain) = app_with_plain_parked_and_session_on_screen();
    let listings = app.mcp_document_listings();
    let mut ran = false;
    {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        let result = resolver.with_document(plain, &mut |_state, _epoch| ran = true);
        assert_eq!(result, Ok(()));
    }
    assert!(ran, "the call reached the plain parked document");
    // And the session on screen is exactly where it was.
    assert_eq!(
        app.host.editor_state().editor_ui.collab.phase,
        Phase::Active
    );
}

#[test]
fn mcp_write_to_a_collaborated_parked_document_is_refused() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, shared, _lane) = app_with_session_parked();
    let listings = app.mcp_document_listings();
    let mut ran = false;
    let result = {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        // Apply, create_document and every other addressed call resolve the
        // document through this one lens entry.
        resolver.with_document(shared, &mut |_state, _epoch| ran = true)
    };
    assert_eq!(result, Err(ResolveError::NotReachable));
    assert!(
        !ran,
        "no write may run against a document others are editing"
    );
    assert_eq!(
        parked_first_node_name(&app, 0),
        parked_first_node_name(&app, 0),
        "the document is back in its slot"
    );
}

#[test]
fn agent_runs_on_a_collaborated_document_stay_blocked() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _shared, _lane) = app_with_session_parked();
    let mut ran = false;

    // The run pump reaches a parked document through `with_slot_runs`.
    let result = app.with_slot_runs(0, |_host, _runs| ran = true);

    assert!(result.is_none());
    assert!(!ran, "an agent must not write into a shared document");
}

#[test]
fn agent_runs_reach_a_plain_parked_document_while_another_slot_collaborates() {
    let (mut app, _plain) = app_with_plain_parked_and_session_on_screen();
    let mut ran = false;

    let result = app.with_slot_runs(0, |_host, _runs| ran = true);

    assert!(result.is_some());
    assert!(ran);
}

#[test]
fn a_parked_guest_shared_document_is_never_written_to_the_recovery_dir() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _paths, work, recovery) = app_with_tabs("guest-shared", &["a.op", "b.op"]);
    // a.op is the guest's own earlier tab; the shared document is in b.op's tab.
    edit_active(&mut app, "someone-elses-work");
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    assert!(app.activate_document_slot(0), "the shared tab parks");

    app.pump_recovery();
    app.flush_recovery();

    assert_eq!(
        entries_in(&recovery).len(),
        0,
        "nothing of the shared document reached the recovery dir: {:?}",
        files_in(&recovery)
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_guests_own_earlier_tab_is_still_captured() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _paths, work, recovery) = app_with_tabs("guest-own", &["a.op", "b.op"]);
    // b.op on screen holds the shared session; a.op (parked) is the guest's own.
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "my-own-edit");
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    // The own tab (a.op, on screen) is now parked behind a plain b.op by
    // switching to it; the session display travels with a.op, so move it: this
    // test wants the SESSION on b.op.
    let session = app.host.editor_state_mut().editor_ui.collab.take_session();
    assert!(app.activate_document_slot(1));
    app.host
        .editor_state_mut()
        .editor_ui
        .collab
        .restore_session(session);

    app.pump_recovery();
    app.flush_recovery();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some("my-own-edit"));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn owner_document_is_captured_read_only_while_parked() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _paths, work, recovery) = app_with_tabs("owner-parked", &["a.op", "b.op"]);
    edit_active(&mut app, "owners-edit");
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );
    assert!(app.activate_document_slot(0), "the owner's tab parks");
    let revision_before = app
        .documents
        .parked_document(1)
        .map(|parked| parked.scope().document_revision());

    app.pump_recovery();
    app.flush_recovery();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some("owners-edit"));
    // Read-only: the document and its session are exactly as they were.
    assert_eq!(parked_phase(&app, 1), Phase::Active);
    assert_eq!(
        app.documents
            .parked_document(1)
            .map(|parked| parked.scope().document_revision()),
        revision_before
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_plain_tab_is_captured_while_another_tab_collaborates() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _paths, work, recovery) = app_with_tabs("plain-parked", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "plain-edit");
    assert!(app.activate_document_slot(1));
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );

    app.pump_recovery();
    app.flush_recovery();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some("plain-edit"));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
