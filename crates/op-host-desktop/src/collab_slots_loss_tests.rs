//! A collaborated guest tab that is not on screen cannot show its panel
//! notice, so a session that ends or keeps failing is shown on the tab
//! (status `Error`) and announced once with a toast naming the tab.

use super::test_support::*;
use crate::collab_avatar_host::lock_avatar_test_registry;
use crate::DesktopApp;
use op_collab_host::test_support;
use op_editor_core::{CollabConnectionPhase as Phase, CollabUiRole, DocumentTabStatus};

const LOST_KEY: &str = "documents.collabSessionLostParked";

/// A guest session in tab 0, parked behind a plain second tab.
fn app_with_a_parked_guest() -> DesktopApp {
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
    app
}

fn tab_status(app: &mut DesktopApp, index: usize) -> DocumentTabStatus {
    app.sync_active_slot();
    let live_dirty = app.document_is_dirty();
    app.documents.tab_views(live_dirty, "Untitled")[index].status
}

fn lose_the_link_of_the_parked_guest(app: &mut DesktopApp) {
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
}

#[test]
fn parked_session_failure_marks_the_tab_error_and_toasts_once() {
    let _avatars = lock_avatar_test_registry();
    let mut app = app_with_a_parked_guest();
    assert_eq!(tab_status(&mut app, 0), DocumentTabStatus::Idle);
    assert!(app.host.editor_state().editor_ui.editor_toast.is_none());

    lose_the_link_of_the_parked_guest(&mut app);

    assert_eq!(parked_phase(&app, 0), Phase::Reconnecting);
    assert_eq!(
        tab_status(&mut app, 0),
        DocumentTabStatus::Error,
        "the parked tab shows that its session is in trouble"
    );
    assert_eq!(
        tab_status(&mut app, 1),
        DocumentTabStatus::Idle,
        "the tab on screen is untouched"
    );
    let toast = app
        .host
        .editor_state()
        .editor_ui
        .editor_toast
        .clone()
        .expect("one toast tells the user");
    assert_eq!(toast.i18n_key, LOST_KEY);
    assert_eq!(toast.args.len(), 1);
    assert_eq!(toast.args[0].0, "name");
    assert!(!toast.args[0].1.is_empty(), "it names the tab");

    // Later polls of the same failure do not announce it again.
    app.host.editor_state_mut().editor_ui.dismiss_toast();
    app.poll_collaboration();
    app.poll_collaboration();
    assert!(
        app.host.editor_state().editor_ui.editor_toast.is_none(),
        "one toast per loss"
    );
}

#[test]
fn a_parked_session_that_ends_marks_the_tab_error_and_toasts_once() {
    let _avatars = lock_avatar_test_registry();
    let mut app = app_with_a_parked_guest();
    app.documents
        .slot_mut(0)
        .unwrap()
        .parked
        .as_mut()
        .expect("parked")
        .scope_mut()
        .collab_mut()
        .set_phase(Phase::Ended);

    app.poll_collaboration();

    assert_eq!(tab_status(&mut app, 0), DocumentTabStatus::Error);
    let toast = app.host.editor_state().editor_ui.editor_toast.clone();
    assert_eq!(toast.expect("announced").i18n_key, LOST_KEY);
}

#[test]
fn a_live_or_reconnecting_parked_session_shows_no_error_before_a_failure_is_reported() {
    let _avatars = lock_avatar_test_registry();
    let mut app = app_with_a_parked_guest();
    assert_eq!(tab_status(&mut app, 0), DocumentTabStatus::Idle, "active");

    // The link dropped but no attempt has failed yet: reconnecting, no notice.
    app.documents
        .slot_mut(0)
        .unwrap()
        .parked
        .as_mut()
        .expect("parked")
        .scope_mut()
        .collab_mut()
        .set_phase(Phase::Reconnecting);

    assert_eq!(tab_status(&mut app, 0), DocumentTabStatus::Idle);
    assert!(app.host.editor_state().editor_ui.editor_toast.is_none());
}

#[test]
fn a_session_that_fails_while_its_tab_is_on_screen_keeps_the_panel_notice_only() {
    let _avatars = lock_avatar_test_registry();
    let mut app = app_with_a_parked_guest();
    assert!(app.activate_document_slot(0));
    app.host
        .editor_state_mut()
        .editor_ui
        .collab
        .set_phase(Phase::Ended);

    app.poll_collaboration();

    assert_eq!(tab_status(&mut app, 0), DocumentTabStatus::Idle);
    assert!(
        app.host.editor_state().editor_ui.editor_toast.is_none(),
        "the on-screen tab has its own panel notice"
    );
}
