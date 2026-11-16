//! A collaboration JOIN never replaces a document that has content.
//!
//! The runtime cannot reach `Joining` in a test (it needs a ticket issuer the
//! open-source build lacks), so these tests inject the runtime step: the fake
//! does what `start_guest_route` does at the moment it matters (consume the
//! queued action, set the phase) and records what was on screen when it ran.
//! The owner's Snapshot is then installed through the real host seam.

use super::*;
use op_collab_host::CollabHost;
use op_editor_core::{
    AuthenticatedCollabSession, CollabConnectionPhase, CollabUiAction, CollabUiRole, DocumentId,
    EditOrigin, EditorState,
};
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;

const OWNER_NODE: &str = "owner:1";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-join-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn owner_snapshot() -> op_editor_core::PenDocument {
    serde_json::from_value(serde_json::json!({
        "version": "1.0",
        "children": [{
            "type": "rectangle", "id": OWNER_NODE, "name": "Owner rect",
            "x": 0, "y": 0, "width": 10, "height": 10
        }]
    }))
    .expect("valid test document")
}

/// What the stand-in runtime saw when it was called.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    active: Option<DocumentId>,
    had_path: bool,
    tabs: usize,
}

/// A bound, dirty document: the guest's own work that a join must not touch.
fn app_with_unsaved_work(tag: &str) -> (DesktopApp, PathBuf, PathBuf) {
    let dir = temp_dir(tag);
    let path = dir.join("mine.op");
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(path.clone()));
    app.host.editor_state_mut().mark_document_changed();
    assert!(app.document_is_dirty());
    (app, path, dir)
}

fn queue_join(app: &mut DesktopApp) {
    app.host.editor_state_mut().editor_ui.collab.pending_action =
        Some(CollabUiAction::JoinAddress {
            endpoint: "127.0.0.1:9".into(),
        });
}

/// Run one frame of the collaboration step with a stand-in runtime that
/// starts the join.
fn join_frame(app: &mut DesktopApp) -> Seen {
    let mut seen = Seen::default();
    app.pump_collaboration_transition_with(false, |app| {
        seen = Seen {
            active: Some(app.documents.active_id()),
            had_path: app.current_path.is_some(),
            tabs: app.documents.len(),
        };
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        let _ = collab.take_pending_action();
        collab.set_phase(CollabConnectionPhase::Joining);
        true
    });
    seen
}

/// The owner's Snapshot arrives. Generic over `CollabHost` exactly like the
/// runtime's own calls, so it reaches the GUI host's override.
fn install_through_seam(host: &mut impl CollabHost) {
    host.install_collaboration_document(owner_snapshot(), EditOrigin::Snapshot)
        .expect("snapshot installs");
    host.mark_editor_state_dirty();
}

fn install_snapshot(app: &mut DesktopApp) {
    install_through_seam(&mut app.host);
}

fn shows_owner_document(app: &DesktopApp) -> bool {
    serde_json::to_string(&app.host.editor_state().doc)
        .expect("serializes")
        .contains(OWNER_NODE)
}

/// The runtime reached `phase` with the session live (post-admission).
fn session_in(app: &mut DesktopApp, phase: CollabConnectionPhase) {
    assert!(app
        .host
        .editor_state_mut()
        .editor_ui
        .collab
        .set_authenticated_session(
            phase,
            AuthenticatedCollabSession {
                session_name: "Shared design".into(),
                role: CollabUiRole::Editor,
                share_endpoint: None,
            },
            Vec::new(),
        ));
}

fn queue(app: &mut DesktopApp, action: CollabUiAction) {
    app.host.editor_state_mut().editor_ui.collab.pending_action = Some(action);
}

/// Run one frame whose stand-in runtime only counts how often it was called.
fn frame_counting_calls(app: &mut DesktopApp) -> usize {
    let mut calls = 0;
    app.pump_collaboration_transition_with(false, |app| {
        calls += 1;
        let _ = app
            .host
            .editor_state_mut()
            .editor_ui
            .collab
            .take_pending_action();
        true
    });
    calls
}

fn phase(app: &DesktopApp) -> CollabConnectionPhase {
    app.host.editor_state().editor_ui.collab.phase
}

fn end_session(app: &mut DesktopApp) {
    let collab = &mut app.host.editor_state_mut().editor_ui.collab;
    collab.set_phase(CollabConnectionPhase::Idle);
    collab.clear_authenticated();
}

#[test]
fn joining_with_unsaved_work_opens_a_new_tab_and_keeps_the_original() {
    let (mut app, path, dir) = app_with_unsaved_work("keep");
    let original = app.documents.active_id();
    let before = serde_json::to_string(&app.host.editor_state().doc).expect("serializes");
    queue_join(&mut app);

    join_frame(&mut app);
    install_snapshot(&mut app);

    // The shared document is in a NEW untitled tab ...
    assert_eq!(app.documents.len(), 2, "the join opened a tab");
    assert_ne!(app.documents.active_id(), original);
    assert!(shows_owner_document(&app));
    assert!(app.current_path.is_none(), "the shared tab is untitled");

    // ... and the guest's own document is exactly as it was.
    end_session(&mut app);
    let index = app
        .documents
        .index_of(original)
        .expect("original still open");
    assert!(app.activate_document_slot(index));
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert!(app.document_is_dirty(), "unsaved work is still unsaved");
    assert_eq!(
        serde_json::to_string(&app.host.editor_state().doc).expect("serializes"),
        before
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn snapshot_never_installs_into_the_previous_document() {
    let (mut app, path, dir) = app_with_unsaved_work("never");
    let original = app.documents.active_id();
    queue_join(&mut app);

    let seen = join_frame(&mut app);
    // The runtime is entered only once the join's own tab is on screen.
    assert_ne!(seen.active, Some(original), "runtime ran on the old tab");
    assert!(!seen.had_path, "runtime ran with the old file on screen");

    install_snapshot(&mut app);
    // The document that holds the guest's file never saw the Snapshot.
    assert_eq!(app.documents.active_id(), seen.active.expect("recorded"));
    assert!(app.current_path.is_none());
    assert!(shows_owner_document(&app));
    assert!(path.is_file());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn joining_from_a_pristine_untitled_tab_reuses_it() {
    let mut app = DesktopApp::new(None);
    assert!(app.active_document_is_pristine_untitled());
    let blank = app.documents.active_id();
    queue_join(&mut app);

    let seen = join_frame(&mut app);
    install_snapshot(&mut app);

    assert_eq!(seen.tabs, 1, "no extra tab for a blank document");
    assert_eq!(seen.active, Some(blank));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), blank);
    assert!(shows_owner_document(&app));
    assert!(app.guest_join_tab.is_none(), "nothing to clean up later");
}

#[test]
fn join_is_refused_when_a_tab_cannot_be_created_and_nothing_changes() {
    let (mut app, path, dir) = app_with_unsaved_work("refused");
    let original = app.documents.active_id();
    let before = serde_json::to_string(&app.host.editor_state().doc).expect("serializes");
    // A clipboard paste decode would deliver into whichever tab is showing.
    let (_tx, rx) = std::sync::mpsc::channel();
    app.pending_figma_paste = Some((0, rx));
    queue_join(&mut app);

    assert_eq!(frame_counting_calls(&mut app), 0, "the runtime never ran");

    assert_eq!(phase(&app), CollabConnectionPhase::Idle);
    assert!(app
        .host
        .editor_state()
        .editor_ui
        .collab
        .pending_action
        .is_none());
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), original);
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert!(app.document_is_dirty());
    assert_eq!(
        serde_json::to_string(&app.host.editor_state().doc).expect("serializes"),
        before
    );
    assert!(app.guest_join_tab.is_none());
    let toast = app.host.editor_state().editor_ui.editor_toast.clone();
    assert_eq!(
        toast.map(|toast| toast.i18n_key),
        Some(JOIN_BLOCKED_KEY.to_string())
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancelled_join_closes_the_unused_new_tab() {
    let (mut app, path, dir) = app_with_unsaved_work("cancel");
    let original = app.documents.active_id();
    queue_join(&mut app);
    join_frame(&mut app);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(phase(&app), CollabConnectionPhase::Joining);

    // The user cancels: the runtime leaves and goes back to Idle.
    queue(&mut app, CollabUiAction::Cancel);
    app.pump_collaboration_transition_with(false, |app| {
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        let _ = collab.take_pending_action();
        collab.set_phase(CollabConnectionPhase::Idle);
        true
    });
    assert!(app.settle_guest_join_tab());

    assert_eq!(app.documents.len(), 1, "no stray tab");
    assert_eq!(app.documents.active_id(), original);
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert!(app.document_is_dirty());
    assert!(app.guest_join_tab.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_join_cancelled_after_the_snapshot_keeps_the_shared_tab() {
    let (mut app, _path, dir) = app_with_unsaved_work("late-cancel");
    queue_join(&mut app);
    join_frame(&mut app);
    install_snapshot(&mut app);
    let shared = app.documents.active_id();

    // Leave before the next frame ever saw the Snapshot.
    end_session(&mut app);
    assert!(!app.settle_guest_join_tab());

    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents.active_id(), shared);
    assert!(shows_owner_document(&app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_join_does_not_leave_a_stray_tab() {
    // The REAL runtime: the open-source build has no ticket issuer, so the
    // join is refused (`AuthenticationUnavailable`) after the tab was opened.
    let (mut app, path, dir) = app_with_unsaved_work("failed");
    let original = app.documents.active_id();
    queue_join(&mut app);

    assert!(app.pump_collaboration_transition());
    assert_eq!(app.documents.len(), 2, "the tab was opened before the join");
    assert!(app.guest_join_tab.is_some());
    let _ = app.collab_runtime.poll(&mut app.host);
    assert_eq!(phase(&app), CollabConnectionPhase::Idle, "the join failed");
    assert!(app.settle_guest_join_tab());

    assert_eq!(app.documents.len(), 1, "no stray tab");
    assert_eq!(app.documents.active_id(), original);
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert!(app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reconnect_does_not_create_another_tab() {
    let (mut app, _path, dir) = app_with_unsaved_work("reconnect");
    queue_join(&mut app);
    join_frame(&mut app);
    // Admitted, but the Snapshot has not arrived yet: Retry resumes in place.
    app.host
        .editor_state_mut()
        .editor_ui
        .collab
        .set_phase(CollabConnectionPhase::Authenticating);
    queue(&mut app, CollabUiAction::Retry);
    assert_eq!(frame_counting_calls(&mut app), 1);
    assert_eq!(app.documents.len(), 2);
    assert!(app.guest_join_tab.is_some(), "still the join's own tab");

    // Snapshot lands, then the link drops and the guest retries.
    install_snapshot(&mut app);
    session_in(&mut app, CollabConnectionPhase::Reconnecting);
    assert!(!app.settle_guest_join_tab());
    assert!(app.guest_join_tab.is_none());
    let shared = app.documents.active_id();
    queue(&mut app, CollabUiAction::Retry);
    assert_eq!(frame_counting_calls(&mut app), 1);
    assert_eq!(app.documents.len(), 2, "Retry opened no tab");
    assert_eq!(app.documents.active_id(), shared);

    // A brand-new join from a live session is refused, not swapped in.
    queue_join(&mut app);
    assert_eq!(frame_counting_calls(&mut app), 0);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.documents.active_id(), shared);
    assert!(shows_owner_document(&app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shared_document_tab_is_untitled_and_save_goes_through_save_as() {
    let (mut app, path, dir) = app_with_unsaved_work("save-as");
    let bytes_before = std::fs::read(&path).expect("file exists");
    queue_join(&mut app);
    join_frame(&mut app);
    install_snapshot(&mut app);
    session_in(&mut app, CollabConnectionPhase::Active);

    assert!(app.current_path.is_none(), "the shared tab has no file");
    assert!(crate::persistence::requires_background_save_as_fork(
        app.host.editor_state()
    ));
    assert!(matches!(
        crate::persistence::handle_save_as(&mut app.host, &mut app.current_path, None),
        crate::persistence::SaveActionOutcome::BackgroundForkRequired
    ));
    assert!(app
        .save_session
        .latest_target(app.host.document_epoch())
        .is_none());

    // The owner leaves; the guest keeps an independent, still untitled fork.
    session_in(&mut app, CollabConnectionPhase::Ended);
    end_session(&mut app);
    assert!(app.current_path.is_none());
    assert_eq!(app.documents.len(), 2);
    assert_eq!(std::fs::read(&path).expect("file exists"), bytes_before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn owner_start_is_unchanged() {
    let (mut app, path, dir) = app_with_unsaved_work("owner");
    let original = app.documents.active_id();
    for action in [CollabUiAction::Start, CollabUiAction::StartLan] {
        queue(&mut app, action);
        assert_eq!(frame_counting_calls(&mut app), 1, "the runtime ran");
        assert_eq!(
            app.documents.len(),
            1,
            "the owner shares the local document"
        );
        assert_eq!(app.documents.active_id(), original);
        assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
        assert!(app.guest_join_tab.is_none());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recovery_still_covers_the_guests_original_tab() {
    use crate::recovery_host::test_support::{
        app_with_tabs, edit_active, entries_in, snapshot_name,
    };
    let (mut app, _paths, work, recovery) = app_with_tabs("join-recovery", &["mine.op"]);
    edit_active(&mut app, "my unsaved edit");
    assert!(entries_in(&recovery).is_empty(), "nothing written yet");

    queue_join(&mut app);
    join_frame(&mut app);
    // Written before the tab was parked, while the original was on screen.
    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        snapshot_name(&entries[0]).as_deref(),
        Some("my unsaved edit")
    );

    // Under the live session the original is parked and the freeze refuses
    // to lend it to the periodic writer; that entry must survive the pumps.
    install_snapshot(&mut app);
    session_in(&mut app, CollabConnectionPhase::Active);
    app.pump_recovery();
    app.pump_recovery();
    app.recovery.wait_idle(std::time::Duration::from_secs(5));
    let names: Vec<_> = entries_in(&recovery)
        .iter()
        .filter_map(snapshot_name)
        .collect();
    assert!(
        names.iter().any(|name| name == "my unsaved edit"),
        "the original's entry survived: {names:?}"
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_refused_join_does_not_cancel_a_running_import() {
    let (mut app, path, dir) = app_with_unsaved_work("import");
    let page = dir.join("page.html");
    std::fs::write(&page, "<html><body><p>imported</p></body></html>").expect("html fixture");
    app.current_html_import = Some(crate::html_import_session::spawn(&mut app.host, page));
    queue_join(&mut app);

    assert_eq!(frame_counting_calls(&mut app), 0, "the runtime never ran");

    assert!(
        app.current_html_import.is_some(),
        "the import was not cancelled"
    );
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert_eq!(phase(&app), CollabConnectionPhase::Idle);
    assert!(app
        .host
        .editor_state()
        .editor_ui
        .collab
        .pending_action
        .is_none());
    crate::html_import_session::cancel(&mut app.host, &mut app.current_html_import);
    let _ = std::fs::remove_dir_all(dir);
}
