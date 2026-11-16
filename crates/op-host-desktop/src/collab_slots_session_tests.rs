//! One session per process, and what closing, quitting and exiting do to a
//! session that lives in a document.

use super::test_support::*;
use crate::collab_avatar_host::lock_avatar_test_registry;
use crate::collab_join_target_host::GuestJoinTab;
use crate::message_dialog::Choice;
use crate::DesktopApp;
use op_collab_host::test_support;
use op_editor_core::{CollabConnectionPhase as Phase, CollabUiAction, CollabUiRole};

/// An app whose first tab holds an owner's session, parked, with a plain tab on
/// screen. The lane guard keeps the runtime's outbound lane connected.
fn app_with_parked_owner(others: usize) -> (DesktopApp, test_support::OwnerLaneGuard) {
    let probe = DesktopApp::new(None);
    let (runtime, lane) = test_support::owner_session(probe.host.editor_state().doc.clone());
    drop(probe);
    // The runtime was built over the starter document every fresh app shows.
    let mut app = DesktopApp::new(None);
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Owner,
        others,
    );
    assert!(app.open_untitled_tab());
    (app, lane)
}

fn queue(app: &mut DesktopApp, action: CollabUiAction) {
    app.host.editor_state_mut().editor_ui.collab.pending_action = Some(action);
}

/// Run one frame's worth of the transition pump with a runtime step that must
/// never be reached; `true` when it was.
fn runtime_step_reached(app: &mut DesktopApp) -> bool {
    let mut reached = false;
    app.pump_collaboration_transition_with(false, |_| {
        reached = true;
        true
    });
    reached
}

fn toast_key(app: &DesktopApp) -> Option<String> {
    app.host
        .editor_state()
        .editor_ui
        .editor_toast
        .as_ref()
        .map(|toast| toast.i18n_key.clone())
}

#[test]
fn second_session_is_refused_while_another_slot_has_one() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    let tabs = app.documents.len();

    for action in [
        CollabUiAction::Start,
        CollabUiAction::StartLan,
        CollabUiAction::BeginDiscovery,
        CollabUiAction::JoinAddress {
            endpoint: "10.0.0.9:7".into(),
        },
    ] {
        app.host.editor_state_mut().editor_ui.editor_toast = None;
        queue(&mut app, action.clone());

        assert!(
            !runtime_step_reached(&mut app),
            "{action:?} reached the runtime"
        );

        assert_eq!(
            toast_key(&app).as_deref(),
            Some("documents.collabAlreadyActive"),
            "{action:?}"
        );
        assert!(
            app.host
                .editor_state()
                .editor_ui
                .collab
                .pending_action
                .is_none(),
            "the refused request does not repeat every frame"
        );
        assert_eq!(app.documents.len(), tabs, "a refused join opens no tab");
    }
    assert_eq!(
        parked_phase(&app, 0),
        Phase::Active,
        "the session is untouched"
    );
}

#[test]
fn a_cancel_on_an_idle_tab_does_not_reach_a_runtime_while_another_slot_collaborates() {
    // Any `leave` rotates the process-wide avatar epoch, which would blank the
    // live session's avatars.
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    queue(&mut app, CollabUiAction::Cancel);

    assert!(!runtime_step_reached(&mut app));
    assert!(app
        .host
        .editor_state()
        .editor_ui
        .collab
        .pending_action
        .is_none());
    assert_eq!(toast_key(&app), None, "a silent drop");
}

#[test]
fn a_session_can_start_when_no_other_slot_has_one() {
    let mut app = DesktopApp::new(None);
    queue(&mut app, CollabUiAction::Start);
    assert!(runtime_step_reached(&mut app));
}

#[test]
fn closing_a_collaborated_tab_asks_to_leave_and_no_keeps_it() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    let mut asked = Vec::new();

    let closed = app.close_document_tab_with(0, |title, body| {
        asked.push((title.to_string(), body.to_string()));
        Some(Choice::No)
    });

    assert!(!closed);
    assert_eq!(asked.len(), 1);
    assert_eq!(app.documents.len(), 2, "nothing changed");
    assert_eq!(parked_phase(&app, 0), Phase::Active, "still in the session");

    // Yes ends the session and closes the tab.
    let closed = app.close_document_tab_with(0, |_, _| Some(Choice::Yes));
    assert!(closed);
    assert_eq!(app.documents.len(), 1);
}

#[test]
fn closing_a_guests_tab_asks_whether_to_leave() {
    let _avatars = lock_avatar_test_registry();
    let mut app = DesktopApp::new(None);
    let baseline = app.host.editor_state().doc.clone();
    let (runtime, _lane) = test_support::guest_session_with_pending_remote_commit(
        baseline.clone(),
        renamed(&baseline, "R"),
    );
    app.collab_runtime = runtime;
    give_session(
        &mut app.host.editor_state_mut().editor_ui.collab,
        Phase::Active,
        CollabUiRole::Editor,
        1,
    );
    assert!(app.open_untitled_tab());
    let mut asked = Vec::new();

    app.close_document_tab_with(0, |title, body| {
        asked.push((title.to_string(), body.to_string()));
        Some(Choice::No)
    });

    assert_eq!(asked.len(), 1);
    assert!(asked[0].1.contains("Leave the session?"), "{}", asked[0].1);
    assert!(
        !asked[0].1.contains("{{"),
        "no placeholder is left: {}",
        asked[0].1
    );
}

#[test]
fn closing_an_owner_tab_warns_about_participant_count() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    // Three other people are in the session (the roster the tab shows).
    give_session(
        app.documents
            .slot_mut(0)
            .and_then(|slot| slot.parked.as_mut())
            .expect("the owner's tab is parked")
            .scope_mut()
            .collab_mut(),
        Phase::Active,
        CollabUiRole::Owner,
        3,
    );
    let mut asked = Vec::new();

    app.close_document_tab_with(0, |title, body| {
        asked.push((title.to_string(), body.to_string()));
        Some(Choice::No)
    });

    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].1.contains("ends the session for 3 participant(s)"),
        "{}",
        asked[0].1
    );
    assert!(!asked[0].1.contains("{{"), "{}", asked[0].1);
}

#[test]
fn closing_a_plain_tab_asks_nothing_about_a_session() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    // The plain tab (on screen) closes without a session question.
    let mut asked = 0;
    assert!(app.close_document_tab_with(1, |_, _| {
        asked += 1;
        Some(Choice::Yes)
    }));
    assert_eq!(asked, 0);
}

#[test]
fn quit_warns_once_and_cancel_aborts() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    let mut asked = 0;

    assert!(!app.confirm_close_with(|_, body| {
        asked += 1;
        assert!(body.contains("1 participant(s)"), "{body}");
        Some(Choice::Cancel)
    }));
    assert_eq!(asked, 1, "one warning");
    assert!(!app.recovery.quit_confirmed, "the quit did not go ahead");
    assert_eq!(
        parked_phase(&app, 0),
        Phase::Active,
        "the session is untouched"
    );

    asked = 0;
    assert!(app.confirm_close_with(|_, _| {
        asked += 1;
        Some(Choice::Yes)
    }));
    assert_eq!(asked, 1, "still one warning, however many tabs there are");
}

#[test]
fn quit_with_no_session_asks_nothing() {
    let mut app = DesktopApp::new(None);
    assert!(app.confirm_close_with(|_, _| panic!("no question without a session")));
}

#[test]
fn exiting_leaves_every_runtime_including_a_parked_one() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    assert_eq!(parked_phase(&app, 0), Phase::Active);

    app.leave_every_session_on_exit();

    assert_eq!(
        parked_phase(&app, 0),
        Phase::Idle,
        "the parked session was left through the lens"
    );
    let runtime = app
        .documents
        .slot_mut(0)
        .unwrap()
        .local
        .collab_runtime
        .as_mut()
        .expect("the runtime stays in its slot");
    assert!(
        runtime
            .drain_status_events()
            .any(|event| matches!(event, op_collab_host::CollabStatusEvent::SessionEnded)),
        "the runtime went through its explicit leave path"
    );
}

#[test]
fn join_handoff_uses_the_target_slots_epoch() {
    // The join's own tab is judged by ITS slot, parked or not.
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    let previous = app.documents.active_id();
    assert!(app.open_untitled_tab());
    let join_tab = app.documents.active_id();
    let tab_epoch = app.host.document_epoch();
    // The user is looking at their own tab again; the join's tab is parked.
    assert!(app.activate_document_slot(0));
    let parked_epoch = app.documents.parked_document(1).unwrap().epoch();
    assert_eq!(parked_epoch, tab_epoch, "the parked slot keeps its epoch");

    // The Snapshot installed (the slot's epoch moved): the tab is the shared
    // document and stays.
    app.guest_join_tab = Some(GuestJoinTab {
        document: join_tab,
        previous,
        epoch: tab_epoch - 1,
    });
    assert!(!app.settle_guest_join_tab());
    assert!(app.guest_join_tab.is_none());
    assert_eq!(app.documents.len(), 2);

    // The join ended without a Snapshot (epoch unchanged, session idle): the
    // empty tab is closed although it is not on screen.
    app.guest_join_tab = Some(GuestJoinTab {
        document: join_tab,
        previous,
        epoch: tab_epoch,
    });
    assert!(app.settle_guest_join_tab());
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), previous);
}

#[test]
fn a_join_still_in_progress_in_a_parked_tab_is_left_alone() {
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    let previous = app.documents.active_id();
    assert!(app.open_untitled_tab());
    let join_tab = app.documents.active_id();
    let tab_epoch = app.host.document_epoch();
    assert!(app.activate_document_slot(0));
    // The join's tab is parked while its session is still being admitted.
    app.documents
        .slot_mut(1)
        .and_then(|slot| slot.parked.as_mut())
        .expect("the join tab is parked")
        .scope_mut()
        .collab_mut()
        .set_phase(Phase::Joining);
    app.guest_join_tab = Some(GuestJoinTab {
        document: join_tab,
        previous,
        epoch: tab_epoch,
    });

    assert!(!app.settle_guest_join_tab());

    assert!(app.guest_join_tab.is_some(), "still tracking the join");
    assert_eq!(app.documents.len(), 2, "the tab is not closed under a join");
}

#[test]
fn save_of_a_collaborated_parked_slot_goes_through_the_fork_path() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    assert!(
        !crate::persistence::requires_background_save_as_fork(app.host.editor_state()),
        "the plain tab on screen saves normally"
    );

    // Saving a parked document always happens on screen: bring it forward.
    assert!(app.activate_document_slot(0));

    assert!(
        crate::persistence::requires_background_save_as_fork(app.host.editor_state()),
        "the collaborated document is saved through the fork path"
    );
}

#[test]
fn save_of_a_plain_slot_is_normal_while_another_collaborates() {
    let _avatars = lock_avatar_test_registry();
    let (mut app, _lane) = app_with_parked_owner(1);
    assert!(!crate::persistence::requires_background_save_as_fork(
        app.host.editor_state()
    ));
    assert!(app.host.gate_collaboration_action(
        op_editor_core::CollabGateAction::SaveShared,
        op_editor_core::CollabEditSource::User,
    ));
}
