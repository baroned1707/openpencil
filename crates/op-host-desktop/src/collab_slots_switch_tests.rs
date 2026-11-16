//! Switching tabs during a session, and what a parked session can and cannot
//! do. Wiring is exercised against `op_collab_host::test_support` runtimes; the
//! protocol itself is covered by that crate's own tests.

use super::test_support::*;
use crate::collab_avatar_host::lock_avatar_test_registry;
use crate::document_registry::BlockReason;
use crate::DesktopApp;
use op_collab_host::test_support;
use op_editor_core::{CollabConnectionPhase as Phase, CollabUiRole, PenNodeExt};

const ESTABLISHED: [Phase; 4] = [
    Phase::Active,
    Phase::Reconnecting,
    Phase::ReadOnly,
    Phase::Ended,
];

#[test]
fn switching_tabs_is_allowed_during_an_established_session() {
    for phase in ESTABLISHED {
        let mut app = DesktopApp::new(None);
        give_session(
            &mut app.host.editor_state_mut().editor_ui.collab,
            phase,
            CollabUiRole::Editor,
            1,
        );
        assert_eq!(app.document_switch_blocked(), None, "{phase:?}");
        assert!(app.open_untitled_tab(), "{phase:?}");

        // The session stayed with its document and the new tab is plain.
        assert_eq!(parked_phase(&app, 0), phase, "{phase:?}");
        assert_eq!(
            app.host.editor_state().editor_ui.collab.phase,
            Phase::Idle,
            "{phase:?}"
        );
        // And back again: the session is still there.
        assert!(app.activate_document_slot(0), "{phase:?}");
        assert_eq!(
            app.host.editor_state().editor_ui.collab.phase,
            phase,
            "{phase:?}"
        );
    }
}

#[test]
fn reconnecting_does_not_block_switching() {
    let mut app = DesktopApp::new(None);
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Reconnecting,
        CollabUiRole::Editor,
        1,
    );
    assert_eq!(app.document_switch_blocked(), None);
}

#[test]
fn switch_is_blocked_while_the_session_is_still_starting_or_joining() {
    for phase in [Phase::Starting, Phase::Joining, Phase::Authenticating] {
        let mut app = DesktopApp::new(None);
        app.host
            .editor_state_mut()
            .editor_ui
            .collab
            .set_phase(phase);
        assert_eq!(
            app.document_switch_blocked(),
            Some(BlockReason::Collaboration),
            "{phase:?}"
        );
        assert!(!app.open_untitled_tab(), "{phase:?}");
        assert_eq!(app.documents.len(), 1, "{phase:?}");
    }
}

#[test]
fn switch_is_blocked_while_a_local_edit_is_in_flight() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let (runtime, _lane) = test_support::owner_session(app.host.editor_state().doc.clone());
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );
    assert_eq!(app.document_switch_blocked(), None);

    assert!(app.collab_runtime.begin_local_edit(&mut app.host));
    assert_eq!(
        app.document_switch_blocked(),
        Some(BlockReason::Collaboration)
    );
    assert!(!app.open_untitled_tab(), "no switch under an open capture");

    app.collab_runtime.finish_local_edit(&mut app.host);
    assert_eq!(app.document_switch_blocked(), None);
}

#[test]
fn a_documents_runtime_stays_with_it_and_the_shared_one_follows_the_screen() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let (runtime, _lane) = test_support::owner_session(app.host.editor_state().doc.clone());
    assert!(runtime.peer_namespace().is_some(), "a live session");
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );

    assert!(app.open_untitled_tab());
    // The session's runtime is parked with its document; the screen has a fresh
    // idle one.
    assert!(app
        .documents
        .slot_mut(0)
        .unwrap()
        .local
        .collab_runtime
        .is_some());
    assert!(app.collab_runtime.peer_namespace().is_none());

    // A third, plain tab: the shared runtime simply follows the screen.
    assert!(app.open_untitled_tab());
    assert!(app
        .documents
        .slot_mut(1)
        .unwrap()
        .local
        .collab_runtime
        .is_none());
    assert!(app
        .documents
        .slot_mut(0)
        .unwrap()
        .local
        .collab_runtime
        .is_some());

    // Back to the session's tab: its own runtime serves the screen again.
    assert!(app.activate_document_slot(0));
    assert!(app.collab_runtime.peer_namespace().is_some());
    assert!(app
        .documents
        .slot_mut(0)
        .unwrap()
        .local
        .collab_runtime
        .is_none());
}

#[test]
fn parked_collaborated_slot_applies_a_remote_commit() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let baseline = app.host.editor_state().doc.clone();
    let (runtime, _lane) = test_support::guest_session_with_pending_remote_commit(
        baseline.clone(),
        renamed(&baseline, "Remote"),
    );
    let mut app = {
        app.collab_runtime = runtime;
        give_session(
            &mut app.host.editor_state_mut().editor_ui.collab,
            Phase::Active,
            CollabUiRole::Editor,
            1,
        );
        assert!(app.open_untitled_tab());
        app
    };
    assert_ne!(parked_first_node_name(&app, 0).as_deref(), Some("Remote"));

    app.poll_collaboration();

    assert_eq!(
        parked_first_node_name(&app, 0).as_deref(),
        Some("Remote"),
        "the remote commit landed in the parked document"
    );
    assert_eq!(parked_phase(&app, 0), Phase::Active);
}

#[test]
fn parked_poll_leaves_the_onscreen_document_untouched() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let baseline = app.host.editor_state().doc.clone();
    let (runtime, _lane) = test_support::guest_session_with_pending_remote_commit(
        baseline.clone(),
        renamed(&baseline, "Remote"),
    );
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    assert!(app.open_untitled_tab());
    let before = on_screen_fingerprint(&app);
    let name_on_screen = app.host.editor_state().doc.children[0].base().name.clone();

    app.poll_collaboration();

    assert_eq!(parked_first_node_name(&app, 0).as_deref(), Some("Remote"));
    assert_eq!(
        on_screen_fingerprint(&app),
        before,
        "the screen is as it was"
    );
    assert_eq!(
        app.host.editor_state().doc.children[0].base().name,
        name_on_screen
    );
}

#[test]
fn parked_poll_requests_no_redraw_unless_phase_changes() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let baseline = app.host.editor_state().doc.clone();
    let (runtime, _lane) = test_support::guest_session_with_pending_remote_commit(
        baseline.clone(),
        renamed(&baseline, "Remote"),
    );
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    assert!(app.open_untitled_tab());

    // A remote commit changes the parked document, which nobody sees.
    assert!(
        !app.poll_collaboration(),
        "streaming into a parked tab is silent"
    );
    assert_eq!(parked_first_node_name(&app, 0).as_deref(), Some("Remote"));

    // A dropped link changes the session's phase, which the tab strip shows.
    let runtime = app
        .documents
        .slot_mut(0)
        .unwrap()
        .local
        .collab_runtime
        .as_ref()
        .expect("the parked session's runtime");
    test_support::queue_transport_loss(runtime);
    assert!(
        app.poll_collaboration(),
        "a phase change asks for a repaint"
    );
    assert_eq!(parked_phase(&app, 0), Phase::Reconnecting);
}

#[test]
fn ticket_renewal_runs_for_a_parked_slot() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let (runtime, lane) = test_support::owner_session(app.host.editor_state().doc.clone());
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );
    assert!(app.open_untitled_tab());
    let _ = lane.drain_command_count(); // whatever parking itself sent

    test_support::queue_ticket_renewal(
        app.documents
            .slot_mut(0)
            .unwrap()
            .local
            .collab_runtime
            .as_ref()
            .expect("the parked session's runtime"),
    );
    app.poll_collaboration();

    assert!(
        lane.drain_command_count() >= 1,
        "the renewed credential reached the peer although the tab is parked"
    );
}

#[test]
fn half_typed_text_reaches_peers_before_the_slot_parks() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let (runtime, lane) = test_support::owner_session(app.host.editor_state().doc.clone());
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );
    // The peers already have this peer's presence, so parking sends none: any
    // command after this point is the committed text.
    app.collab_runtime
        .publish_local_presence(&mut app.host, None);
    let _ = lane.drain_command_count();
    // A layer rename the user is still typing.
    let node = op_editor_core::NodeId::new(app.host.editor_state().doc.children[0].id_str());
    assert!(app.host.editor_state_mut().start_rename_layer(node));
    assert!(app.host.editor_state_mut().rename_append("Typed"));
    assert_eq!(lane.drain_command_count(), 0, "nothing sent yet");

    assert!(app.open_untitled_tab());

    let name = parked_first_node_name(&app, 0).expect("the parked node");
    assert!(name.ends_with("Typed"), "the draft was committed: {name}");
    assert!(
        lane.drain_command_count() >= 1,
        "the committed text was sent to the peer, not left in a parked document"
    );
}

#[test]
fn presence_is_cleared_when_the_slot_parks() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let (runtime, lane) = test_support::owner_session(app.host.editor_state().doc.clone());
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        1,
    );
    // The peers last saw a cursor.
    assert!(app
        .collab_runtime
        .publish_local_presence(&mut app.host, Some((10.0, 20.0))));
    let _ = lane.drain_command_count();

    assert!(app.open_untitled_tab());

    assert_eq!(
        lane.drain_command_count(),
        1,
        "exactly one cursor-less presence frame went out as the slot parked"
    );
}

#[test]
fn a_parked_reconnecting_guest_gets_a_wake_at_its_retry_deadline() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let baseline = app.host.editor_state().doc.clone();
    let (runtime, _lane) = test_support::guest_session_with_pending_remote_commit(
        baseline.clone(),
        renamed(&baseline, "Remote"),
    );
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    assert!(app.open_untitled_tab());
    assert_eq!(app.next_collab_deadline(), None, "nothing is waiting");

    // The link drops while the tab is parked.
    test_support::queue_transport_loss(
        app.documents
            .slot_mut(0)
            .unwrap()
            .local
            .collab_runtime
            .as_ref()
            .expect("the parked session's runtime"),
    );
    app.poll_collaboration();

    assert!(
        app.next_collab_deadline().is_some(),
        "the idle event loop must wake for the parked guest's retry"
    );
}
