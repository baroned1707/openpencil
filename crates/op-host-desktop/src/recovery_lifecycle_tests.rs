//! The ends of a run: the snapshot before an agent starts, the one at exit,
//! the quit prompts, and the offer to recover at the next launch.

use crate::message_dialog::Choice;
use crate::recovery_host::test_support::*;
use crate::recovery_store::SnapshotReason;
use crate::DesktopApp;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

fn set_modified(path: &Path, when: SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(when))
        .expect("set mtime");
}

fn hours_ago(hours: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(hours * 3600)
}

/// A previous run (launch `Lprev`, a process that no longer exists) that had
/// `a.op` open with unsaved edits plus an untitled document with edits, and
/// died without asking. Returns the path of `a.op`.
fn crashed_previous_run(work: &Path, recovery: &Path) -> PathBuf {
    let mut previous = app_with_recovery(recovery, "Lprev", DEAD_PID);
    let path = write_document(work, "a.op");
    set_modified(&path, hours_ago(2));
    assert!(previous.open_document_tab(path.clone()));
    edit_active(&mut previous, "recovered-a");
    assert!(previous.open_untitled_tab());
    edit_active(&mut previous, "recovered-untitled");
    previous.recovery_on_exit();
    assert_eq!(entries_in(recovery).len(), 2, "{:?}", files_in(recovery));
    path
}

/// Run startup recovery in a fresh app, answering `choice`. Returns the app,
/// the number recovered and the dialog `(title, body)`.
fn recover_with(
    recovery: &Path,
    choice: Option<Choice>,
) -> (DesktopApp, usize, Option<(String, String)>) {
    let mut app = app_with_recovery(recovery, "Lnew", std::process::id());
    // The dialog text is localized; the assertions below read the English.
    app.host.editor_state_mut().editor_ui.locale = op_editor_core::Locale::EnUs;
    let mut asked = None;
    let recovered = app.offer_recovery_with(|title, body| {
        asked = Some((title.to_string(), body.to_string()));
        choice
    });
    (app, recovered, asked)
}

#[test]
fn pre_run_snapshot_is_taken_before_the_run_starts() {
    let (mut app, _paths, work, recovery) = app_with_tabs("prerun", &["a.op"]);
    // An hour between periodic snapshots: only the pre-run trigger can write.
    app.recovery.set_interval(Duration::from_secs(3600));
    edit_active(&mut app, "before-the-agent");
    app.pump_recovery();
    app.flush_recovery();
    assert!(
        files_in(&recovery).is_empty(),
        "the periodic rule has not fired"
    );

    // Nothing waiting to be sent: no snapshot, however often this runs.
    app.launch_chat_if_pending();
    assert!(files_in(&recovery).is_empty());

    // A send is waiting: the document is on disk when the launch runs.
    app.host.editor_state_mut().chat.pending_send = Some("make a card".to_string());
    app.launch_chat_if_pending();
    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(entries[0].manifest.reason, SnapshotReason::PreRun);
    assert_eq!(
        snapshot_name(&entries[0]).as_deref(),
        Some("before-the-agent")
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn pre_run_snapshot_covers_untitled_content_but_not_a_clean_file() {
    let recovery = temp_dir("prerun-untitled");
    let mut app = app_with_recovery(&recovery, "Lrun1", std::process::id());
    // Untitled, content, marked saved (as a generated document is): clean, but
    // there is no file to fall back on.
    edit_active(&mut app, "generated");
    app.host.editor_state_mut().mark_saved_revision();
    assert!(!app.document_is_dirty());
    assert_eq!(
        app.snapshot_dirty_documents(SnapshotReason::Autosnapshot),
        0
    );

    assert_eq!(app.snapshot_before_run(), 1);
    assert_eq!(
        snapshot_name(&entries_in(&recovery)[0]).as_deref(),
        Some("generated")
    );

    // A clean document that has its file needs nothing.
    let work = temp_dir("prerun-untitled-work");
    let clean = temp_dir("prerun-clean");
    let mut app = app_with_recovery(&clean, "Lrun2", std::process::id());
    assert!(app.open_document_tab(write_document(&work, "a.op")));
    assert_eq!(app.snapshot_before_run(), 0);
    assert!(files_in(&clean).is_empty());
    for dir in [recovery, work, clean] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn exit_snapshot_writes_every_dirty_slot_within_budget() {
    let (mut app, paths, work, recovery) = app_with_tabs("exit", &["a.op", "b.op", "c.op"]);
    // a stays clean; b (parked) and c (parked) and a new untitled tab are dirty.
    assert!(app.activate_document_slot(1));
    edit_active(&mut app, "edit-b");
    assert!(app.activate_document_slot(2));
    edit_active(&mut app, "edit-c");
    assert!(app.open_untitled_tab());
    edit_active(&mut app, "edit-untitled");
    // An hour between periodic snapshots: this is the exit path's work alone.
    app.recovery.set_interval(Duration::from_secs(3600));

    let started = Instant::now();
    app.recovery_on_exit();
    let elapsed = started.elapsed();

    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 3, "{:?}", files_in(&recovery));
    assert!(entries
        .iter()
        .all(|entry| entry.manifest.reason == SnapshotReason::Exit));
    let mut names: Vec<_> = entries.iter().filter_map(snapshot_name).collect();
    names.sort();
    assert_eq!(names, vec!["edit-b", "edit-c", "edit-untitled"]);
    let mut originals: Vec<_> = entries
        .iter()
        .map(|entry| entry.manifest.original_path.clone())
        .collect();
    originals.sort();
    assert_eq!(
        originals,
        vec![
            None,
            Some(paths[1].to_string_lossy().into_owned()),
            Some(paths[2].to_string_lossy().into_owned())
        ]
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_confirmed_quit_writes_nothing_and_leaves_no_entries() {
    let (mut app, _paths, work, recovery) = app_with_tabs("confirmed", &["a.op", "b.op"]);
    edit_active(&mut app, "edit-b");
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "edit-a");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(entries_in(&recovery).len(), 2);

    // Don't Save for both: the user threw the edits away on purpose.
    assert!(app.confirm_close_with(|_, _| Some(Choice::No)));
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    app.recovery_on_exit();

    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn an_edit_made_after_the_save_prompt_survives_a_confirmed_quit() {
    let (mut app, _paths, work, recovery) = app_with_tabs("late-edit", &["a.op", "b.op"]);
    edit_active(&mut app, "edit-b");
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "edit-a");
    // a: Save. b: Don't Save.
    let mut question = 0;
    assert!(app.confirm_close_with(|_, _| {
        question += 1;
        Some(if question == 1 {
            Choice::Yes
        } else {
            Choice::No
        })
    }));
    // Stopping an agent finalizes its design after the prompts. Both
    // documents change again: a was saved, b was thrown away on purpose.
    edit_active(&mut app, "edit-b-final");
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "agent-final-a");

    app.recovery_on_exit();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(entries[0].manifest.reason, SnapshotReason::Exit);
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some("agent-final-a"));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn cancelling_the_quit_keeps_every_entry() {
    let (mut app, _paths, work, recovery) = app_with_tabs("cancelled", &["a.op", "b.op"]);
    edit_active(&mut app, "edit-b");
    assert!(app.activate_document_slot(0));
    edit_active(&mut app, "edit-a");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(entries_in(&recovery).len(), 2);

    // Don't Save for the first document, Cancel at the second.
    let mut question = 0;
    let quit = app.confirm_close_with(|_, _| {
        question += 1;
        Some(if question == 1 {
            Choice::No
        } else {
            Choice::Cancel
        })
    });

    assert!(!quit);
    assert_eq!(entries_in(&recovery).len(), 2, "{:?}", files_in(&recovery));
    // The app keeps running; if it then dies unasked, both are written again.
    app.recovery_on_exit();
    assert_eq!(entries_in(&recovery).len(), 2);
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn startup_offers_recovery_and_reopens_dirty_tabs_bound_to_original_path() {
    let work = temp_dir("startup-work");
    let recovery = temp_dir("startup-store");
    let original = crashed_previous_run(&work, &recovery);

    let (mut app, recovered, asked) = recover_with(&recovery, Some(Choice::Yes));

    let (title, body) = asked.expect("one confirmation");
    assert_eq!(title, "Recover 2 unsaved documents?");
    assert!(body.contains("a.op") && body.contains("Untitled"), "{body}");
    assert_eq!(recovered, 2);
    assert_eq!(app.documents.len(), 2, "the blank launch tab made room");
    // a.op comes back bound to its file, with the unsaved edit, still unsaved.
    let index = app
        .documents
        .index_of_path(&original)
        .expect("bound to a.op");
    assert!(app.activate_document_slot(index));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    assert!(app.document_is_dirty());
    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some("recovered-a")
    );
    // Save writes the recovered content back to the original file. The file's
    // state when the edits were made is unknown, so the first save asks once.
    let mut ui = crate::save_session::conflict::OverwritingConflictUi::default();
    assert!(app.request_background_save_with(&mut ui) && app.finish_background_saves());
    assert_eq!(ui.asked, 1, "a recovered document's first save asks");
    let on_disk =
        op_host_services::doc_io::load_editor_state(&original, op_editor_core::Locale::EnUs)
            .expect("saved file loads");
    assert_eq!(on_disk.doc.name.as_deref(), Some("recovered-a"));
    // The untitled one is untitled and unsaved.
    let untitled = (0..app.documents.len())
        .find(|&i| app.documents.slot_paths()[i].is_none())
        .expect("an untitled tab");
    assert!(app.activate_document_slot(untitled));
    assert!(app.document_is_dirty());
    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some("recovered-untitled")
    );
    // The previous run's entries were replaced by this run's own (the untitled
    // tab's is still needed; the saved a.op's was cleared by the save).
    app.pump_recovery();
    app.flush_recovery();
    let entries = entries_in(&recovery);
    assert!(
        entries
            .iter()
            .all(|entry| entry.manifest.launch_id == "Lnew"),
        "{entries:?}"
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(
        snapshot_name(&entries[0]).as_deref(),
        Some("recovered-untitled")
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn recovery_binds_no_path_that_another_tab_already_has_open() {
    let work = temp_dir("open-elsewhere-work");
    let recovery = temp_dir("open-elsewhere-store");
    let original = crashed_previous_run(&work, &recovery);
    let mut app = app_with_recovery(&recovery, "Lnew", std::process::id());
    app.host.editor_state_mut().editor_ui.locale = op_editor_core::Locale::EnUs;
    // The user opened a.op themselves before answering the offer.
    assert!(app.open_document_tab(original.clone()));

    let recovered = app.offer_recovery_with(|_, _| Some(Choice::Yes));

    assert_eq!(recovered, 2);
    app.sync_active_slot();
    let bound = app
        .documents
        .slot_paths()
        .into_iter()
        .flatten()
        .filter(|path| path == &original)
        .count();
    assert_eq!(bound, 1, "a second tab must not be bound to the same file");
    // The recovered edits are not lost: they sit in an untitled tab.
    let recovered_copy = (0..app.documents.len())
        .find(|&index| {
            app.documents.slot_paths()[index].is_none()
                && app.activate_document_slot(index)
                && app.host.editor_state().doc.name.as_deref() == Some("recovered-a")
        })
        .is_some();
    assert!(recovered_copy, "the recovered a.op edits open untitled");
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn recovery_binds_no_path_when_the_original_is_newer() {
    let work = temp_dir("newer-work");
    let recovery = temp_dir("newer-store");
    let original = crashed_previous_run(&work, &recovery);
    // The file was saved by something else after the snapshot was taken.
    set_modified(&original, SystemTime::now() + Duration::from_secs(120));
    let before = std::fs::read(&original).expect("original");

    let (mut app, recovered, asked) = recover_with(&recovery, Some(Choice::Yes));

    assert_eq!(recovered, 2);
    let (_, body) = asked.expect("asked");
    assert!(body.contains("a.op (the file on disk is newer"), "{body}");
    assert!(
        app.documents.index_of_path(&original).is_none(),
        "no tab is bound to the newer file"
    );
    assert!(app.documents.slot_paths().iter().all(Option::is_none));
    // Both are unsaved copies carrying their edits.
    let mut names = Vec::new();
    for index in 0..app.documents.len() {
        assert!(app.activate_document_slot(index));
        assert!(app.document_is_dirty());
        names.push(app.host.editor_state().doc.name.clone().unwrap_or_default());
    }
    names.sort();
    assert_eq!(names, vec!["recovered-a", "recovered-untitled"]);
    assert_eq!(std::fs::read(&original).unwrap(), before, "never touched");
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_missing_original_opens_as_an_untitled_copy() {
    let work = temp_dir("missing-work");
    let recovery = temp_dir("missing-store");
    let original = crashed_previous_run(&work, &recovery);
    std::fs::remove_file(&original).expect("remove original");

    let (app, recovered, asked) = recover_with(&recovery, Some(Choice::Yes));

    assert_eq!(recovered, 2);
    assert!(asked.expect("asked").1.contains("original file is gone"));
    assert!(app.documents.slot_paths().iter().all(Option::is_none));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn discard_deletes_entries() {
    let work = temp_dir("decline-work");
    let recovery = temp_dir("decline-store");
    crashed_previous_run(&work, &recovery);

    let (app, recovered, asked) = recover_with(&recovery, Some(Choice::No));

    assert!(asked.is_some());
    assert_eq!(recovered, 0);
    assert_eq!(app.documents.len(), 1, "only the blank launch tab");
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn no_answer_recovers_and_cancel_decides_later() {
    let work = temp_dir("answers-work");
    let recovery = temp_dir("answers-store");
    crashed_previous_run(&work, &recovery);

    // Cancel: nothing opens and nothing is deleted.
    let (app, recovered, _) = recover_with(&recovery, Some(Choice::Cancel));
    assert_eq!((recovered, app.documents.len()), (0, 1));
    assert_eq!(entries_in(&recovery).len(), 2);

    // No dialog could be shown (`None`): unsaved work is never discarded unseen.
    let (app, recovered, _) = recover_with(&recovery, None);
    assert_eq!((recovered, app.documents.len()), (2, 2));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[cfg(unix)]
#[test]
fn entries_of_a_live_other_instance_are_ignored() {
    let work = temp_dir("live-work");
    let recovery = temp_dir("live-store");
    // Another instance (here: a process that is really running) with unsaved
    // work and a snapshot of it.
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let mut running = app_with_recovery(&recovery, "Lother", other.id());
    assert!(running.open_document_tab(write_document(&work, "a.op")));
    edit_active(&mut running, "live-edit");
    running.recovery_on_exit();
    assert_eq!(entries_in(&recovery).len(), 1);

    // This launch must neither offer nor delete it.
    let mut app = app_with_recovery(&recovery, "Lnew", std::process::id());
    let recovered = app.offer_recovery_with(|_, _| panic!("a live instance's entry was offered"));
    assert_eq!(recovered, 0);
    assert_eq!(entries_in(&recovery).len(), 1, "left in place");

    // Once that process is gone the same entry is up for recovery.
    other.kill().expect("kill");
    other.wait().expect("reap");
    let recovered = app.offer_recovery_with(|_, _| Some(Choice::Yes));
    assert_eq!(recovered, 1);
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn startup_recovery_survives_unusable_entries() {
    let work = temp_dir("junk-work");
    let recovery = temp_dir("junk-store");
    crashed_previous_run(&work, &recovery);
    std::fs::write(recovery.join("Lprev-77.json"), b"{ broken").unwrap();
    std::fs::write(recovery.join("Lprev-77.op"), b"{}").unwrap();
    std::fs::write(recovery.join("notes.txt"), b"mine").unwrap();

    let (app, recovered, _) = recover_with(&recovery, Some(Choice::Yes));

    assert_eq!(recovered, 2);
    assert_eq!(app.documents.len(), 2);
    assert!(recovery.join("notes.txt").exists());
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
