//! A Home brief swaps a fresh page into the tab on screen. What the shell does
//! with the document it replaced: unsaved work must stay reachable as a tab.

use crate::DesktopApp;
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;

const UNSAVED_NAME: &str = "unsaved design";

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-home-swap-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::canonicalize(dir).expect("canonical temp dir")
}

fn app_with_file(dir: &std::path::Path) -> (DesktopApp, PathBuf) {
    let mut app = DesktopApp::new(None);
    let path = dir.join("design.op");
    save_to_path(&op_editor_core::EditorState::starter(), &path).expect("fixture saves");
    assert!(app.open_document_tab(path.clone()));
    (app, path)
}

fn edit(app: &mut DesktopApp) {
    let state = app.host.editor_state_mut();
    state.doc.name = Some(UNSAVED_NAME.to_string());
    state.mark_document_changed();
}

fn brief_replaces_the_document(app: &mut DesktopApp) {
    assert!(app.host.start_fresh_document_for_home());
    assert!(app.drain_replaced_home_document());
}

#[test]
fn home_brief_swap_keeps_unsaved_work_in_its_own_tab() {
    let dir = temp_dir("dirty-file");
    let (mut app, path) = app_with_file(&dir);
    edit(&mut app);

    brief_replaces_the_document(&mut app);

    assert_eq!(app.documents.len(), 2, "the replaced work is a tab");
    assert!(app.current_path.is_none(), "the new design is untitled");
    assert!(!app.document_is_dirty(), "the new design is clean");
    assert_ne!(
        app.host.editor_state().doc.name.as_deref(),
        Some(UNSAVED_NAME)
    );
    app.sync_active_slot();
    let kept = app
        .documents
        .index_of_path(&path)
        .expect("the work is still bound to its own file");
    assert_ne!(kept, app.documents.active_index());
    assert!(app.documents.is_dirty(kept, false), "still unsaved");
    assert!(app.activate_document_slot(kept));
    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some(UNSAVED_NAME)
    );
    assert!(app.document_is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn home_brief_swap_keeps_unsaved_untitled_work_in_its_own_untitled_tab() {
    let mut app = DesktopApp::new(None);
    edit(&mut app);

    brief_replaces_the_document(&mut app);

    assert_eq!(app.documents.len(), 2);
    assert!(app.current_path.is_none());
    assert!(!app.document_is_dirty());
    let untitled_parked = (0..app.documents.len())
        .filter(|&index| index != app.documents.active_index())
        .any(|index| app.documents.is_dirty(index, false));
    assert!(untitled_parked, "the unsaved untitled work is a dirty tab");
}

#[test]
fn home_brief_swap_of_a_saved_document_opens_no_extra_tab() {
    let dir = temp_dir("saved-file");
    let (mut app, path) = app_with_file(&dir);

    brief_replaces_the_document(&mut app);

    assert_eq!(app.documents.len(), 1, "nothing unsaved, nothing to keep");
    assert!(
        app.current_path.is_none(),
        "the new design never inherits the path"
    );
    assert!(path.is_file(), "the saved file is untouched");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn home_brief_swap_leaves_exactly_one_recovery_entry_for_the_kept_work() {
    use crate::recovery_host::test_support::*;
    let (mut app, _paths, work, recovery) = app_with_tabs("home-swap", &["a.op"]);
    edit_active(&mut app, UNSAVED_NAME);
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(entries_in(&recovery).len(), 1, "the edit was snapshotted");

    // No periodic pump between the swap and a crash: the kept tab must already
    // have its own snapshot, and the old document id's entry must be gone.
    brief_replaces_the_document(&mut app);
    app.flush_recovery();

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1, "{:?}", files_in(&recovery));
    assert_eq!(snapshot_name(&entries[0]).as_deref(), Some(UNSAVED_NAME));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}
