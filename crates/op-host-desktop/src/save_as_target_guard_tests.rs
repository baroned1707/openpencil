//! Save As never binds a second tab to a file another tab has open.

use crate::DesktopApp;
use op_editor_core::EditorCommand;
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-saveas-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::canonicalize(dir).expect("canonical temp dir")
}

/// `a.op` (parked) and `b.op` (on screen), both clean on disk.
fn two_tabs(tag: &str) -> (DesktopApp, PathBuf, PathBuf, PathBuf) {
    let dir = temp_dir(tag);
    let mut app = DesktopApp::new(None);
    let mut paths = Vec::new();
    for name in ["a.op", "b.op"] {
        let path = dir.join(name);
        save_to_path(&op_editor_core::EditorState::starter(), &path).expect("fixture saves");
        assert!(app.open_document_tab(path.clone()));
        paths.push(path);
    }
    let path_b = paths.pop().unwrap();
    let path_a = paths.pop().unwrap();
    (app, path_a, path_b, dir)
}

/// Give the on-screen document content its file does not have.
fn rename_a_node_in_the_active_document(app: &mut DesktopApp) {
    assert!(app
        .host
        .editor_state_mut()
        .apply(EditorCommand::UpdateNode {
            node_id: op_editor_core::NodeId::new("n10".to_string()),
            x: None,
            y: None,
            width: None,
            height: None,
            name: Some("edited in tab b".into()),
            fill_hex: None,
            page_id: None,
        }));
}

#[test]
fn save_as_onto_a_file_open_in_another_tab_is_refused_and_nothing_is_written() {
    let (mut app, path_a, _path_b, dir) = two_tabs("refused");
    rename_a_node_in_the_active_document(&mut app);
    let before = std::fs::read(&path_a).expect("a.op");

    let started = app.save_as_to_picked_path(path_a.clone(), false);
    app.finish_background_saves();

    assert!(!started, "Save As onto another tab's file must be refused");
    assert_eq!(
        std::fs::read(&path_a).expect("a.op"),
        before,
        "the other tab's file is untouched"
    );
    let toast = app
        .host
        .editor_state()
        .editor_ui
        .editor_toast
        .clone()
        .expect("the refusal is shown");
    assert_eq!(toast.i18n_key, "documents.saveAsAlreadyOpen");
    assert_eq!(toast.args, vec![("name".to_string(), "a.op".to_string())]);
    app.sync_active_slot();
    let bound_to_a = app
        .documents
        .slot_paths()
        .into_iter()
        .flatten()
        .filter(|path| path == &path_a)
        .count();
    assert_eq!(bound_to_a, 1, "no second tab is bound to a.op");
    assert_ne!(app.current_path.as_deref(), Some(path_a.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_as_onto_the_tabs_own_path_is_allowed() {
    let (mut app, _path_a, path_b, dir) = two_tabs("own");
    rename_a_node_in_the_active_document(&mut app);
    let before = std::fs::read(&path_b).expect("b.op");

    let started = app.save_as_to_picked_path(path_b.clone(), false);
    app.finish_background_saves();

    assert!(started, "a tab may be saved over its own file");
    assert_ne!(
        std::fs::read(&path_b).expect("b.op"),
        before,
        "it was written"
    );
    assert!(app.host.editor_state().editor_ui.editor_toast.is_none());
    assert_eq!(app.current_path.as_deref(), Some(path_b.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_as_onto_a_new_name_is_allowed_and_binds_the_tab() {
    let (mut app, _path_a, _path_b, dir) = two_tabs("fresh");
    let fresh = dir.join("fresh.op");

    assert!(app.save_as_to_picked_path(fresh.clone(), false));
    app.finish_background_saves();

    assert!(fresh.is_file());
    assert_eq!(app.current_path.as_deref(), Some(fresh.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}
