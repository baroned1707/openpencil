//! When a headless `DesktopApp` writes and removes crash-recovery entries:
//! the periodic rule, the drag guard, saves and closes.

use super::test_support::*;
use super::*;
use crate::message_dialog::Choice;
use std::time::Duration;

fn only_entry_of(dir: &std::path::Path) -> crate::recovery_store::RecoveryEntry {
    let mut entries = entries_in(dir);
    assert_eq!(entries.len(), 1, "{:?}", files_in(dir));
    entries.remove(0)
}

#[test]
fn dirty_parked_document_is_snapshotted_without_switching_to_it() {
    let (mut app, paths, work, recovery) = app_with_tabs("parked", &["a.op", "b.op"]);
    edit_active(&mut app, "edit-in-b");
    assert!(app.activate_document_slot(0)); // b is parked, a is on screen
    assert!(!app.document_is_dirty(), "a is clean");
    let on_screen = app.documents.active_id();

    app.pump_recovery();
    app.flush_recovery();

    assert_eq!(app.documents.active_id(), on_screen, "no tab switch");
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    let entry = only_entry_of(&recovery);
    assert_eq!(snapshot_name(&entry).as_deref(), Some("edit-in-b"));
    assert_eq!(
        entry.manifest.original_path.as_deref(),
        Some(paths[1].to_string_lossy().as_ref())
    );
    assert!(entry.manifest.base_saved);
    assert_eq!(entry.manifest.label, "b.op");
    assert_eq!(entry.manifest.reason, SnapshotReason::Autosnapshot);
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn clean_document_is_never_snapshotted() {
    let (mut app, _paths, work, recovery) = app_with_tabs("clean", &["a.op", "b.op"]);

    for _ in 0..3 {
        app.pump_recovery();
    }
    app.snapshot_dirty_documents(SnapshotReason::Exit);
    app.flush_recovery();

    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn snapshot_is_skipped_while_a_drag_is_in_progress() {
    let (mut app, _paths, work, recovery) = app_with_tabs("drag", &["a.op"]);
    edit_active(&mut app, "mid-drag");
    // A pan drag with the Hand tool: the same `drag` state a canvas drag holds.
    app.host.editor_state_mut().tool = op_editor_core::Tool::Hand;
    app.host
        .apply_press(700.0, 450.0, app.viewport_width, app.viewport_height);
    assert!(
        app.host.pointer_gesture_in_progress(),
        "the press started a drag"
    );

    app.pump_recovery();
    app.flush_recovery();
    assert!(
        files_in(&recovery).is_empty(),
        "nothing is written mid-drag"
    );
    assert!(
        app.recovery.next_deadline().is_some(),
        "and a retry stays scheduled"
    );

    app.host.apply_release();
    assert!(!app.host.pointer_gesture_in_progress());
    // The retry is a second away; this test does not wait, it asks again once
    // the guard is gone and the retry time is reached.
    for track in app.recovery.tracks.values_mut() {
        track.due = Some(Instant::now());
    }
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(only_entry_of(&recovery).manifest.label, "a.op");
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn snapshot_is_skipped_while_a_text_edit_is_in_progress() {
    let (mut app, _paths, work, recovery) = app_with_tabs("textedit", &["a.op"]);
    edit_active(&mut app, "typing");
    app.host.editor_state_mut().ui.text_editing = Some(op_editor_core::NodeId::new("n10"));
    assert!(app.host.pointer_gesture_in_progress());

    app.pump_recovery();
    app.flush_recovery();

    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn the_periodic_snapshot_waits_for_its_interval_and_wakes_an_idle_app() {
    let (mut app, _paths, work, recovery) = app_with_tabs("interval", &["a.op"]);
    app.recovery.set_interval(Duration::from_secs(10));
    assert!(
        app.recovery.next_deadline().is_none(),
        "nothing unsaved, no wake"
    );
    edit_active(&mut app, "waiting");

    app.pump_recovery();
    app.flush_recovery();

    assert!(files_in(&recovery).is_empty(), "10 s have not passed");
    let due = app.recovery.next_deadline().expect("a wake is armed");
    assert!(due > Instant::now() + Duration::from_secs(8), "{due:?}");
    for track in app.recovery.tracks.values_mut() {
        track.due = Some(Instant::now());
    }
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(
        snapshot_name(&only_entry_of(&recovery)).as_deref(),
        Some("waiting")
    );
    assert!(
        app.recovery.next_deadline().is_none(),
        "snapshotted: no more wakes"
    );

    // An unchanged document is not written again; a new edit is.
    let before = only_entry_of(&recovery).manifest.saved_at_unix;
    app.pump_recovery();
    assert!(app.recovery.next_deadline().is_none());
    edit_active(&mut app, "waiting-more");
    app.pump_recovery();
    assert!(
        app.recovery.next_deadline().is_some(),
        "re-armed by the new edit"
    );
    let _ = before;
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn save_ack_clears_the_documents_entry() {
    let (mut app, paths, work, recovery) = app_with_tabs("ack", &["a.op", "b.op"]);
    edit_active(&mut app, "saved-edit-b");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(only_entry_of(&recovery).manifest.label, "b.op");

    assert!(app.request_background_save());
    assert!(app.finish_background_saves());

    // Straight after the acknowledgement: no tick in between.
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    assert!(!app.document_is_dirty());

    // A parked tab's acknowledgement clears its entry the same way.
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "edit-in-a");
    assert!(app.activate_document_slot(1)); // a is parked and dirty
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(only_entry_of(&recovery).manifest.label, "a.op");
    let (epoch, generation, revision) = {
        let parked = app.documents.slot_mut(0).unwrap().parked.as_ref().unwrap();
        (
            parked.epoch(),
            parked.scope().document_generation(),
            parked.scope().document_revision(),
        )
    };
    assert!(app.apply_parked_save_ack(epoch, generation, revision, Some(paths[0].as_path())));
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn an_edit_after_a_save_is_snapshotted_again() {
    let (mut app, _paths, work, recovery) = app_with_tabs("after-save", &["a.op"]);
    edit_active(&mut app, "first");
    app.pump_recovery();
    app.flush_recovery();
    assert!(app.request_background_save() && app.finish_background_saves());
    assert!(files_in(&recovery).is_empty());

    edit_active(&mut app, "second");
    app.pump_recovery();
    app.flush_recovery();

    assert_eq!(
        snapshot_name(&only_entry_of(&recovery)).as_deref(),
        Some("second")
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn closing_a_tab_with_dont_save_clears_its_entry() {
    let (mut app, _paths, work, recovery) = app_with_tabs("close", &["a.op", "b.op"]);
    edit_active(&mut app, "doomed");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(only_entry_of(&recovery).manifest.label, "b.op");

    // Cancel keeps the tab and its entry.
    assert!(!app.close_document_tab_with(1, |_, _| Some(Choice::Cancel)));
    assert_eq!(app.documents.len(), 2);
    assert_eq!(only_entry_of(&recovery).manifest.label, "b.op");

    // Don't Save closes the tab and takes its entry with it.
    assert!(app.close_document_tab_with(1, |_, _| Some(Choice::No)));
    assert_eq!(app.documents.len(), 1);
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_tab_that_vanishes_takes_its_entry_with_it() {
    let (mut app, _paths, work, recovery) = app_with_tabs("vanish", &["a.op", "b.op"]);
    edit_active(&mut app, "gone");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(files_in(&recovery).len(), 2);

    // Discarded without any prompt (for example by a replace-the-tab flow).
    assert!(app.discard_document_tab(1));
    app.pump_recovery();

    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
