//! Open-document tabs end to end on a headless `DesktopApp`: opening,
//! switching, closing, quitting and save acknowledgements.

use super::*;
use crate::message_dialog::Choice;
use crate::save_session::SaveCompletion;
use op_host_services::doc_io::save_to_path;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-tabs-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Write a one-frame document to `dir/name` and return its path.
fn write_document(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    path
}

fn app_with_tabs(tag: &str, names: &[&str]) -> (DesktopApp, Vec<PathBuf>, PathBuf) {
    let dir = temp_dir(tag);
    let mut app = DesktopApp::new(None);
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| write_document(&dir, name))
        .collect();
    for path in &paths {
        assert!(app.open_document_tab(path.clone()), "{}", path.display());
    }
    (app, paths, dir)
}

fn edit_active(app: &mut DesktopApp) {
    app.host.editor_state_mut().mark_document_changed();
}

#[test]
fn the_first_opened_file_replaces_the_blank_launch_tab() {
    let (app, paths, dir) = app_with_tabs("blank", &["a.op"]);
    assert_eq!(app.documents.len(), 1, "the untouched blank tab made room");
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_same_path_switches_instead_of_duplicating() {
    let (mut app, paths, dir) = app_with_tabs("dup", &["a.op", "b.op"]);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.current_path.as_deref(), Some(paths[1].as_path()));

    assert!(app.open_document_tab(paths[0].clone()));
    assert_eq!(app.documents.len(), 2, "no duplicate tab");
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    // A path that only resolves to the same file still finds the open tab.
    let roundabout = dir.join(".").join("b.op");
    assert!(app.open_document_tab(roundabout));
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.current_path.as_deref(), Some(paths[1].as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn forwarded_open_creates_new_tab_not_replace() {
    let (mut app, paths, dir) = app_with_tabs("forward", &["a.op"]);
    edit_active(&mut app);
    let second = write_document(&dir, "second.op");
    app.forwarded_files
        .lock()
        .unwrap()
        .push_back(second.clone());

    assert!(app.drain_forwarded_files());
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.current_path.as_deref(), Some(second.as_path()));
    // The document that was open is parked with its edits, not thrown away.
    assert!(app.documents.is_dirty(0, false));
    assert_eq!(
        app.documents.session_documents()[0].path,
        paths[0].to_string_lossy()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_restores_each_documents_own_path_and_edits() {
    let (mut app, paths, dir) = app_with_tabs("switch", &["a.op", "b.op"]);
    edit_active(&mut app); // b is dirty
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    assert!(!app.document_is_dirty(), "a is clean");
    assert!(
        app.documents.is_dirty(1, false),
        "b kept its edit while parked"
    );
    assert!(app.activate_document_slot(1));
    assert!(app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn tab_rows_are_mirrored_into_editor_ui() {
    let (mut app, _paths, dir) = app_with_tabs("rows", &["a.op", "b.op"]);
    edit_active(&mut app);
    app.publish_document_tabs();
    let tabs = &app.host.editor_state().editor_ui.document_tabs;
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0].label, "a.op");
    assert!(!tabs[0].active && !tabs[0].dirty);
    assert_eq!(tabs[1].label, "b.op");
    assert!(tabs[1].active && tabs[1].dirty);
    assert!(app.host.editor_state().editor_ui.document_tabs_supported);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_last_tab_leaves_untitled() {
    let (mut app, _paths, dir) = app_with_tabs("last", &["a.op"]);
    assert!(app.close_document_tab_with(0, |_, _| panic!("a clean tab asks nothing")));
    assert_eq!(app.documents.len(), 1, "never zero tabs");
    assert!(app.current_path.is_none(), "the replacement is untitled");
    assert!(!app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_a_dirty_tab_asks_and_cancel_keeps_it() {
    let (mut app, paths, dir) = app_with_tabs("dirtyclose", &["a.op", "b.op"]);
    edit_active(&mut app);
    let mut asked = 0;
    assert!(!app.close_document_tab_with(1, |_, _| {
        asked += 1;
        Some(Choice::Cancel)
    }));
    assert_eq!(asked, 1);
    assert_eq!(app.documents.len(), 2, "cancel keeps the tab");

    assert!(app.close_document_tab_with(1, |_, _| Some(Choice::No)));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn quit_prompts_every_dirty_slot() {
    let (mut app, _paths, dir) = app_with_tabs("quit", &["a.op", "b.op", "c.op"]);
    // Dirty the first and last tab; the middle one stays clean.
    edit_active(&mut app); // c
    assert!(app.activate_document_slot(0));
    edit_active(&mut app); // a
    assert!(app.activate_document_slot(1)); // b on screen, clean

    let mut asked = Vec::new();
    assert!(app.confirm_close_with(|_, body| {
        asked.push(body.to_owned());
        Some(Choice::No)
    }));
    assert_eq!(asked.len(), 2, "one question per dirty tab, none for clean");
    assert!(
        asked[0].contains("a.op") && asked[1].contains("c.op"),
        "{asked:?}"
    );

    // Cancelling stops the quit at the first dirty tab and leaves it on screen.
    let mut asked = 0;
    assert!(!app.confirm_close_with(|_, _| {
        asked += 1;
        Some(Choice::Cancel)
    }));
    assert_eq!(asked, 1);
    assert!(
        app.document_is_dirty(),
        "the tab that stopped the quit is showing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_ack_marks_origin_slot_clean() {
    let (mut app, paths, dir) = app_with_tabs("ack", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0));
    edit_active(&mut app); // a is dirty
    let (epoch, generation, revision) = (
        app.host.document_epoch(),
        app.host.editor_state().document_generation(),
        app.host.editor_state().document_revision(),
    );
    // The user switches away before the background write finishes.
    assert!(app.activate_document_slot(1));
    edit_active(&mut app); // b is dirty too
    assert!(app.documents.is_dirty(0, false));

    app.apply_save_completion(SaveCompletion {
        path: paths[0].clone(),
        set_current_path: false,
        document_epoch: epoch,
        generation,
        revision,
        result: Ok(()),
    });

    assert!(
        !app.documents.is_dirty(0, false),
        "a, which issued the save, is clean"
    );
    assert!(
        app.document_is_dirty(),
        "b, on screen, is untouched by a's ack"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stale_save_ack_for_a_closed_tab_changes_nothing() {
    let (mut app, paths, dir) = app_with_tabs("stale", &["a.op", "b.op"]);
    edit_active(&mut app);
    app.apply_save_completion(SaveCompletion {
        path: paths[0].clone(),
        set_current_path: false,
        document_epoch: u64::MAX,
        generation: 0,
        revision: 0,
        result: Ok(()),
    });
    assert!(app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_swap_while_run_bound() {
    let (mut app, _paths, dir) = app_with_tabs("gate", &["a.op", "b.op"]);
    assert_eq!(app.document_switch_blocked(), None);

    app.host.editor_state_mut().chat.agents_running = (1, 1);
    assert_eq!(
        app.document_switch_blocked(),
        Some(BlockReason::DesignRun),
        "a running design loop forbids switching"
    );
    let active = app.documents.active_index();
    assert!(!app.activate_document_slot(0), "switch is refused");
    assert!(!app.open_untitled_tab(), "a new tab is refused");
    assert!(
        !app.close_document_tab_with(0, |_, _| Some(Choice::No)),
        "close is refused"
    );
    assert_eq!(app.documents.active_index(), active);
    assert_eq!(app.documents.len(), 2);
    assert!(
        app.host.editor_state().editor_ui.editor_toast.is_some(),
        "the refusal is visible"
    );

    app.host.editor_state_mut().chat.agents_running = (0, 0);
    app.save_session = crate::save_session::SaveSession::new();
    assert_eq!(app.document_switch_blocked(), None);
    assert!(app.activate_document_slot(0));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_tab_request_from_the_strip_is_executed_once() {
    let (mut app, paths, dir) = app_with_tabs("request", &["a.op", "b.op"]);
    let first = app.documents.id_at(0).unwrap();
    app.request_document_tab(DocumentTabRequest::Switch(first));
    assert!(app.pump_document_tabs());
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    assert!(app
        .host
        .editor_state()
        .editor_ui
        .pending_document_request
        .is_none());

    // A request naming a tab that is gone is ignored, not misdirected.
    let gone = op_editor_core::DocumentId(9_999);
    app.request_document_tab(DocumentTabRequest::Close(gone));
    app.pump_document_tabs();
    assert_eq!(app.documents.len(), 2);
    app.request_document_tab(DocumentTabRequest::SwitchToIndex(7));
    app.pump_document_tabs();
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn session_restore_reopens_existing_files_and_skips_missing_ones() {
    let (mut app, paths, dir) = app_with_tabs("restore", &["a.op", "b.op"]);
    app.publish_document_tabs();
    let saved = app.host.editor_state().editor_ui.session_documents.clone();
    assert_eq!(saved.len(), 2);

    let mut fresh = DesktopApp::new(None);
    let mut with_missing = saved;
    with_missing.insert(
        0,
        op_editor_core::SessionDocument {
            path: dir.join("gone.op").to_string_lossy().into_owned(),
            active: false,
        },
    );
    with_missing[1].active = true; // a.op was on screen
    with_missing[2].active = false;
    fresh.pending_session_restore = with_missing;
    fresh.restore_session_documents();

    assert_eq!(fresh.documents.len(), 2, "the missing file is skipped");
    assert_eq!(fresh.current_path.as_deref(), Some(paths[0].as_path()));
    let _ = std::fs::remove_dir_all(dir);
}
