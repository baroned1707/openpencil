//! Where a finished import lands: only into a pristine untitled tab, else into
//! a new tab opened first, so an open document is never replaced.

use super::publish::PersistedFile;
use super::*;
use crate::DesktopApp;
use op_host_services::doc_io::save_to_path;
use std::time::{Duration, Instant};

const HTML_PAGE: &str = "<html><body><p>imported</p></body></html>";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!(
        "op-import-tab-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_html(dir: &Path) -> PathBuf {
    let path = dir.join("page.html");
    std::fs::write(&path, HTML_PAGE).expect("write html fixture");
    path
}

fn open_saved_tab(app: &mut DesktopApp, dir: &Path) -> PathBuf {
    let path = dir.join("work.op");
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    assert!(app.open_document_tab(path.clone()));
    path
}

/// Pump both import sessions until none is left (or fail after a deadline).
fn run_imports_to_completion(app: &mut DesktopApp) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.current_figma_import.is_some() || app.current_html_import.is_some() {
        app.pump_import_sessions();
        assert!(Instant::now() < deadline, "import never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A figma session whose worker already delivered `name` as its result.
fn finished_figma_session(dir: &Path, name: &str) -> FigmaImportSession {
    let mut state = EditorState::starter();
    state.doc.name = Some(name.to_string());
    let (tx, rx) = mpsc::channel();
    tx.send(Ok(CompletedImport {
        prepared: PreparedImport {
            state,
            warnings: Vec::new(),
        },
        persisted: Ok(PersistedFile::new(dir.join("Design.op"))),
    }))
    .expect("queue import result");
    FigmaImportSession {
        path: dir.join("Design.fig"),
        stage: SessionStage::Converting(rx),
        cancellation: CancellationToken::default(),
        output_mode: ImportOutputMode::CreateFixed,
    }
}

fn is_imported_html(app: &DesktopApp) -> bool {
    app.current_path.is_none() && app.host.editor_state().is_dirty()
}

#[test]
fn importing_into_a_dirty_tab_opens_a_new_tab_and_keeps_the_original() {
    let dir = temp_dir("dirty");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    app.host.editor_state_mut().mark_document_changed();
    let baseline = app.host.editor_state().doc.clone();

    assert!(app.begin_html_import(write_html(&dir)));
    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2, "the import got its own tab");
    assert!(is_imported_html(&app), "the import is on screen");
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    assert_eq!(app.host.editor_state().doc, baseline);
    assert!(app.document_is_dirty(), "its unsaved edit is still there");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn importing_into_a_saved_clean_tab_also_opens_a_new_tab() {
    let dir = temp_dir("saved");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);

    assert!(app.begin_html_import(write_html(&dir)));
    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2);
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn importing_into_a_pristine_untitled_tab_reuses_it() {
    let dir = temp_dir("pristine");
    let mut app = DesktopApp::new(None);
    assert!(app.active_document_is_pristine_untitled());
    let tab = app.documents.active_id();

    assert!(app.begin_html_import(write_html(&dir)));
    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 1, "no extra tab");
    assert_eq!(app.documents.active_id(), tab, "same tab, new content");
    assert!(is_imported_html(&app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dropping_an_import_file_never_replaces_unsaved_work() {
    let dir = temp_dir("drop");
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    let baseline = app.host.editor_state().doc.clone();

    app.on_dropped_file(write_html(&dir));
    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2);
    assert!(app.activate_document_slot(0));
    assert_eq!(app.host.editor_state().doc, baseline);
    assert!(
        app.document_is_dirty(),
        "the unsaved edit survived the drop"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn second_launch_import_forward_opens_a_new_tab() {
    let dir = temp_dir("forward");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    app.forwarded_files
        .lock()
        .unwrap()
        .push_back(write_html(&dir));

    assert!(app.drain_forwarded_files());
    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2);
    assert!(is_imported_html(&app));
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_figma_result_for_a_dirty_tab_lands_in_a_new_tab() {
    let dir = temp_dir("figma");
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    let baseline = app.host.editor_state().doc.clone();
    app.current_figma_import = Some(finished_figma_session(&dir, "figma result"));

    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2);
    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some("figma result")
    );
    assert_eq!(
        app.current_path.as_deref(),
        Some(dir.join("Design.op").as_path())
    );
    assert!(app.activate_document_slot(0));
    assert_eq!(app.host.editor_state().doc, baseline);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_tabs_during_an_import_is_refused_so_the_result_lands_in_its_own_tab() {
    let dir = temp_dir("switch");
    let mut app = DesktopApp::new(None);
    open_saved_tab(&mut app, &dir);
    let second = dir.join("second.op");
    save_to_path(&EditorState::starter(), &second).expect("fixture saves");
    assert!(app.open_document_tab(second.clone()));
    let origin = app.documents.active_id();
    // A result that has not been collected yet: the import is "in flight".
    app.current_figma_import = Some(finished_figma_session(&dir, "figma result"));

    assert!(!app.activate_document_slot(0), "switching is refused");
    assert!(!app.open_untitled_tab(), "a new tab is refused");
    assert!(!app.close_document_tab_with(0, |_, _| panic!("no question")));
    assert_eq!(app.documents.active_id(), origin);

    run_imports_to_completion(&mut app);
    // `second` was saved and clean, so the result opened ITS OWN tab beside it.
    assert_eq!(app.documents.len(), 3);
    assert!(app.activate_document_slot(1));
    assert_eq!(app.current_path.as_deref(), Some(second.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_import_never_replaces_work_done_while_it_ran() {
    let dir = temp_dir("raced");
    let mut app = DesktopApp::new(None);
    assert!(app.active_document_is_pristine_untitled());
    // The import was requested in a pristine tab ...
    app.current_figma_import = Some(finished_figma_session(&dir, "figma result"));
    // ... and the user drew something before the result arrived.
    app.host.editor_state_mut().mark_document_changed();
    let baseline = app.host.editor_state().doc.clone();

    run_imports_to_completion(&mut app);

    assert_eq!(app.documents.len(), 2, "the late import got a new tab");
    assert!(app.activate_document_slot(0));
    assert_eq!(app.host.editor_state().doc, baseline);
    assert!(app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn blocked_tab_creation_refuses_and_changes_nothing() {
    let dir = temp_dir("blocked");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    let baseline = app.host.editor_state().doc.clone();
    // A clipboard paste decode forbids creating a tab.
    let (_tx, rx) = std::sync::mpsc::channel();
    app.pending_figma_paste = Some((0, rx));

    // At request time: refused up front, no session, no overlay.
    assert!(!app.begin_html_import(write_html(&dir)));
    assert!(app.current_html_import.is_none());
    assert!(!app.host.editor_state().editor_ui.figma_import_in_progress);
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.host.editor_state().doc, baseline);
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    assert!(
        app.host.editor_state().editor_ui.editor_toast.is_some(),
        "the user is told"
    );

    // At result time: the result is dropped, the document untouched.
    app.current_figma_import = Some(finished_figma_session(&dir, "figma result"));
    app.pump_import_sessions();
    assert!(app.current_figma_import.is_none());
    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.host.editor_state().doc, baseline);
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}
