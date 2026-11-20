//! Autosave at the end of an agent run, through the real `DesktopApp` frame
//! pump. Like `document_runs_tests` (this module is nested in it for its
//! fixtures), only the model is replaced by channels the test feeds by hand:
//! a real agent run is not exercised.

use super::*;
use crate::agent_run_autosave::{
    should_autosave_after_run, AutosaveDecision, AutosaveFacts, DeferReason, RunEnd, SkipReason,
};
use op_host_services::disk_fingerprint::DiskFingerprint;

fn lock() -> std::sync::MutexGuard<'static, ()> {
    agent_indicators::test_guard()
}

fn file_names(path: &Path) -> Vec<String> {
    let loaded =
        op_host_services::doc_io::load_editor_state(path, op_editor_core::Locale::EnUs).unwrap();
    document_names(&loaded.doc)
}

fn toast_key(app: &DesktopApp) -> Option<String> {
    app.host
        .editor_state()
        .editor_ui
        .editor_toast
        .as_ref()
        .map(|toast| toast.i18n_key.clone())
}

/// The first frame after the files were opened: their baselines are recorded.
fn first_frame(app: &mut DesktopApp) {
    app.pump_document_runs();
}

fn facts() -> AutosaveFacts {
    AutosaveFacts {
        setting_enabled: true,
        run_end: RunEnd::Succeeded,
        other_run_live: false,
        document_dirty: true,
        bound_to_file: true,
        collaboration_allows_save: true,
        user_request_pending: false,
        save_in_flight: false,
    }
}

/// Invariant: the decision saves only a dirty, file-bound document whose last
/// run succeeded with the setting on, and says why not otherwise.
#[test]
fn autosave_decision_table() {
    use AutosaveDecision::{Defer, Save, Skip};
    let cases: [(&str, AutosaveFacts, AutosaveDecision); 11] = [
        ("all clear", facts(), Save),
        (
            "setting off",
            AutosaveFacts {
                setting_enabled: false,
                ..facts()
            },
            Skip(SkipReason::SettingOff),
        ),
        (
            "errored run",
            AutosaveFacts {
                run_end: RunEnd::Failed,
                ..facts()
            },
            Skip(SkipReason::RunNotSuccessful),
        ),
        (
            "cancelled run",
            AutosaveFacts {
                run_end: RunEnd::Cancelled,
                ..facts()
            },
            Skip(SkipReason::RunNotSuccessful),
        ),
        (
            "another run live",
            AutosaveFacts {
                other_run_live: true,
                ..facts()
            },
            Skip(SkipReason::NotLastRun),
        ),
        (
            "untitled",
            AutosaveFacts {
                bound_to_file: false,
                ..facts()
            },
            Skip(SkipReason::Untitled),
        ),
        (
            "clean",
            AutosaveFacts {
                document_dirty: false,
                ..facts()
            },
            Skip(SkipReason::NothingToSave),
        ),
        (
            "collaboration",
            AutosaveFacts {
                collaboration_allows_save: false,
                ..facts()
            },
            Skip(SkipReason::Collaboration),
        ),
        (
            "user request pending",
            AutosaveFacts {
                user_request_pending: true,
                ..facts()
            },
            Defer(DeferReason::UserRequestPending),
        ),
        (
            "save in flight",
            AutosaveFacts {
                save_in_flight: true,
                ..facts()
            },
            Defer(DeferReason::SaveInFlight),
        ),
        (
            "cancelled and untitled: the run decides first",
            AutosaveFacts {
                run_end: RunEnd::Cancelled,
                bound_to_file: false,
                ..facts()
            },
            Skip(SkipReason::RunNotSuccessful),
        ),
    ];
    for (label, input, expected) in cases {
        assert_eq!(should_autosave_after_run(&input), expected, "{label}");
    }
}

#[test]
fn autosave_runs_when_last_run_of_a_dirty_bound_document_ends_successfully() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-ok", &["a.op"]);
    first_frame(&mut app);
    let run = start_run(&mut app);
    let ack = run.insert("Saved by autosave");
    run.say("working");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("tool ran").is_error);
    assert!(
        app.document_is_dirty(),
        "mid-run the edit is only in memory"
    );
    assert!(!file_names(&paths[0]).contains(&"Saved by autosave".to_string()));

    run.finish();
    app.pump_document_runs();

    assert!(!app.runs.is_live());
    assert!(!app.document_is_dirty(), "autosave cleared the dirty mark");
    assert!(file_names(&paths[0]).contains(&"Saved by autosave".to_string()));
    assert_eq!(
        app.disk_baselines.get(&paths[0]),
        DiskFingerprint::capture(&paths[0]).ok(),
        "the file as autosave left it is the new baseline"
    );
    assert_eq!(toast_key(&app), None, "success is quiet");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosave_skips_cancelled_errored_and_untitled() {
    let _guard = lock();
    // Cancelled: the user stopped the run, so its partial work stays unsaved.
    let (mut app, paths, dir) = app_with_documents("autosave-cancel", &["a.op"]);
    first_frame(&mut app);
    let run = start_run(&mut app);
    let ack = run.insert("Partial");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("tool ran").is_error);
    app.cancel_runs_of_slot(0);
    app.pump_document_runs();
    assert!(app.document_is_dirty());
    assert!(!file_names(&paths[0]).contains(&"Partial".to_string()));
    drop(run);

    // Errored: the run ended with an error message.
    let run = start_run(&mut app);
    let ack = run.insert("Half done");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);
    run.say("error: provider rejected the request");
    run.finish();
    app.pump_document_runs();
    assert!(app.document_is_dirty());
    assert!(!file_names(&paths[0]).contains(&"Half done".to_string()));
    let _ = std::fs::remove_dir_all(dir);

    // Untitled: no file to save to, and nothing is created.
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    assert!(app.current_path.is_none());
    let run = start_run(&mut app);
    run.finish();
    app.pump_document_runs();
    assert!(
        app.document_is_dirty(),
        "an untitled document stays unsaved"
    );
    assert!(app.current_path.is_none());
}

#[test]
fn autosave_waits_for_the_last_live_run() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-last", &["a.op"]);
    first_frame(&mut app);
    let run = start_run(&mut app);
    let ack = run.insert("Edit");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);
    let id = app.documents.active_id();

    // One run "ended" while another run of the same document is still live:
    // not the last run, so nothing is saved.
    app.autosave_after_runs(vec![(id, RunEnd::Succeeded)]);
    assert!(app.document_is_dirty());
    assert!(!file_names(&paths[0]).contains(&"Edit".to_string()));

    // The last run ends: now it is saved.
    run.finish();
    app.pump_document_runs();
    assert!(!app.document_is_dirty());
    assert!(file_names(&paths[0]).contains(&"Edit".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosave_of_a_parked_document_does_not_switch_tabs() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-parked", &["a.op", "b.op"]);
    first_frame(&mut app);
    let run = start_run(&mut app); // in B
    let ack = run.insert("ParkedEdit");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);
    assert!(app.activate_document_slot(0)); // A on screen, B parked with its run
    let epoch = app.host.document_epoch();
    let on_screen = app.documents.active_id();

    run.finish();
    app.pump_document_runs();

    assert_eq!(app.documents.active_id(), on_screen, "no tab switch");
    assert_eq!(app.host.document_epoch(), epoch);
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    assert!(file_names(&paths[1]).contains(&"ParkedEdit".to_string()));
    assert!(!file_names(&paths[0]).contains(&"ParkedEdit".to_string()));
    app.sync_active_slot();
    assert!(
        !app.documents.tab_views(false, "")[1].dirty,
        "the parked tab's unsaved dot is gone"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosave_failure_toasts_and_keeps_document_dirty() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-fail", &["a.op"]);
    first_frame(&mut app);
    let run = start_run(&mut app);
    let ack = run.insert("Unsaved");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);
    // Someone else rewrites the file while the run is going.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&paths[0], b"someone else's work").unwrap();

    run.finish();
    app.pump_document_runs();

    assert!(app.document_is_dirty(), "the agent's work is still unsaved");
    assert_eq!(
        std::fs::read(&paths[0]).unwrap(),
        b"someone else's work",
        "the other change was not overwritten"
    );
    // The toast names the real reason (the file changed on disk) and the file.
    assert_eq!(
        toast_key(&app).as_deref(),
        Some("dialog.autosaveChangedOnDisk"),
        "a failed autosave is never silent"
    );
    let toast = app
        .host
        .editor_state()
        .editor_ui
        .editor_toast
        .clone()
        .unwrap();
    assert_eq!(
        toast.args,
        vec![("name".to_string(), "a.op".to_string())],
        "the toast carries the file name"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autosave_setting_off_never_saves() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-off", &["a.op"]);
    first_frame(&mut app);
    app.host
        .editor_state_mut()
        .editor_ui
        .agent_settings
        .autosave_on_agent_run_end = false;
    let run = start_run(&mut app);
    let ack = run.insert("Stays in memory");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);

    run.finish();
    app.pump_document_runs();

    assert!(app.document_is_dirty());
    assert!(!file_names(&paths[0]).contains(&"Stays in memory".to_string()));
    assert_eq!(toast_key(&app), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn revert_last_agent_run_still_works_after_autosave() {
    let _guard = lock();
    let (mut app, paths, dir) = app_with_documents("autosave-revert", &["a.op"]);
    first_frame(&mut app);
    let before = names_in(app.host.editor_state());
    let run = start_run(&mut app);
    // The checkpoint a send takes right before it launches.
    app.runs.checkpoint = Some(crate::run_checkpoint::RunCheckpoint::capture(&app.host));
    let ack = run.insert("AgentCard");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("ran").is_error);
    run.finish();
    app.pump_document_runs();
    assert!(!app.document_is_dirty(), "autosaved");
    assert!(file_names(&paths[0]).contains(&"AgentCard".to_string()));

    assert!(app.has_revertable_run(), "the checkpoint survived the save");
    let outcome = app.revert_last_agent_run_with(|_title, _body| Some(Choice::Yes));

    assert_eq!(outcome, Ok(crate::run_checkpoint::RevertOutcome::Reverted));
    assert_eq!(names_in(app.host.editor_state()), before);
    assert!(app.document_is_dirty(), "a revert is an edit to save");
    assert!(
        file_names(&paths[0]).contains(&"AgentCard".to_string()),
        "the revert does not touch the disk"
    );
    let _ = std::fs::remove_dir_all(dir);
}
