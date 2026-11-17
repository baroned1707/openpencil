//! The dock follows the disk: what an external change looks like, when a rescan
//! runs, and what a rescan must leave alone. Every case builds its own folders
//! under the OS temp dir.

use super::super::tests::{panel, settle};
use super::super::*;
use op_host_services::doc_io::save_to_path;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn plain_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-refresh-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn write_op(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
    path
}

fn labels(app: &DesktopApp) -> Vec<String> {
    panel(app)
        .iter()
        .flat_map(|project| project.files.iter().map(|file| file.label.clone()))
        .collect()
}

/// An app with `root` opened as a project and its first scan landed.
fn app_with_project(root: &Path) -> DesktopApp {
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(root));
    settle(&mut app);
    app
}

/// What the user does after changing files elsewhere: come back to the window.
fn come_back(app: &mut DesktopApp) -> bool {
    app.on_window_focus_changed(true);
    settle(app)
}

#[test]
fn external_rename_replaces_the_row() {
    let root = plain_dir("rename");
    let old = write_op(&root, "a.op");
    write_op(&root, "b.op");
    let mut app = app_with_project(&root);
    assert_eq!(labels(&app), ["a.op", "b.op"]);

    std::fs::rename(&old, root.join("hero.op")).unwrap();
    assert!(come_back(&mut app), "the tree changed");

    assert_eq!(
        labels(&app),
        ["b.op", "hero.op"],
        "old row gone, new row there"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn external_delete_removes_the_row_and_keeps_an_open_tab() {
    let root = plain_dir("delete");
    let open = write_op(&root, "open.op");
    let closed = write_op(&root, "closed.op");
    let mut app = app_with_project(&root);
    assert!(app.open_document_tab(open.clone()));
    settle(&mut app);
    let tabs_before = app.documents.len();

    std::fs::remove_file(&open).unwrap();
    std::fs::remove_file(&closed).unwrap();
    come_back(&mut app);

    let files = &panel(&app)[0].files;
    assert_eq!(files.len(), 1, "the closed file's row is gone: {files:?}");
    let row = &files[0];
    assert_eq!(row.label, "open.op");
    assert!(row.missing, "the tab's row says it is not on disk");
    assert!(row.open_id.is_some(), "and still belongs to its tab");
    assert!(!row.removable, "a file that is gone cannot be trashed");
    assert_eq!(app.documents.len(), tabs_before, "the tab is never closed");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_tab_opened_on_a_file_outside_the_listing_is_not_marked_missing() {
    // Hidden folders are not walked, so the file is open but never listed.
    let root = plain_dir("hidden");
    write_op(&root, "a.op");
    let hidden = write_op(&root, ".hidden/secret.op");
    let mut app = app_with_project(&root);
    assert!(app.open_document_tab(hidden));
    settle(&mut app);
    come_back(&mut app);
    assert!(
        panel(&app)[0].files.iter().all(|file| !file.missing),
        "being absent from the listing is not being absent from the disk"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// Invariant: a rescan is started by focus regained and by the app's own file
/// operations, and by nothing else that is idle.
#[test]
fn rescan_runs_on_focus_regain_and_after_own_operations() {
    let root = plain_dir("triggers");
    write_op(&root, "a.op");
    let mut app = app_with_project(&root);
    assert!(app.projects_host.scan.is_idle());

    // Focus regained while the dock shows its list: a full rescan is queued.
    assert!(app.projects_dock_visible(), "the headless dock is showing");
    write_op(&root, "from-outside.op");
    app.on_window_focus_changed(true);
    assert!(
        !app.projects_host.scan.is_idle(),
        "focus regain asks for a scan"
    );
    settle(&mut app);
    assert!(labels(&app).contains(&"from-outside.op".to_string()));

    // Losing focus asks for nothing.
    app.on_window_focus_changed(false);
    assert!(app.projects_host.scan.is_idle());

    // An own operation (here: a file made by the app) rescans too, focused or not.
    write_op(&root, "own.op");
    app.rescan_after_own_operation();
    assert!(!app.projects_host.scan.is_idle());
    settle(&mut app);
    assert!(labels(&app).contains(&"own.op".to_string()));
    let _ = std::fs::remove_dir_all(root);
}

/// Invariant: an unfocused app, or one whose dock is folded, schedules no wake
/// and starts no background rescan.
#[test]
fn an_unfocused_app_or_a_folded_dock_asks_for_no_background_work() {
    let root = plain_dir("gating");
    write_op(&root, "a.op");
    let mut app = app_with_project(&root);

    app.on_window_focus_changed(true);
    settle(&mut app);
    assert!(app.projects_background_active());
    assert!(
        app.next_projects_wake().is_some(),
        "focused + dock: a tick is due"
    );

    app.on_window_focus_changed(false);
    assert!(!app.projects_background_active());
    assert!(
        app.next_projects_wake().is_none(),
        "unfocused: the loop sleeps"
    );

    app.on_window_focus_changed(true);
    settle(&mut app);
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .toggle_open();
    assert!(
        !app.projects_background_active(),
        "a folded dock needs no rescan"
    );
    assert!(app.next_projects_wake().is_none());
    let _ = std::fs::remove_dir_all(root);
}

/// Invariant: a rescan that finds a real change rebuilds the rows but leaves
/// what the user is doing in the dock alone: a folded folder, the scroll
/// offset, the open file, an inline rename in progress.
#[test]
fn rescan_preserves_collapse_scroll_selection_and_edit_state() {
    let root = plain_dir("preserve");
    let open = write_op(&root, "designs/open.op");
    write_op(&root, "designs/other.op");
    write_op(&root, "notes/todo.op");
    let mut app = app_with_project(&root);
    assert!(app.open_document_tab(open.clone()));
    settle(&mut app);
    let root_text = root.to_string_lossy().into_owned();
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::ToggleFolder(root_text, "notes".to_string()));
    app.pump_projects();
    settle(&mut app);
    let other = root.join("designs/other.op");
    {
        let panel = &mut app.host.editor_state_mut().editor_ui.projects_panel;
        panel.scroll.offset = 7.0;
        assert!(panel.begin_rename(&other.to_string_lossy(), 0));
        panel.rename.as_mut().unwrap().input.set_text("draft name");
    }

    write_op(&root, "notes/new.op");
    assert!(come_back(&mut app), "a real change");

    assert!(labels(&app).contains(&"new.op".to_string()));
    let project = &panel(&app)[0];
    assert!(project.is_folder_collapsed("notes"), "the fold stays");
    let panel_state = &app.host.editor_state().editor_ui.projects_panel;
    assert_eq!(panel_state.scroll.offset, 7.0, "the scroll stays");
    assert_eq!(
        panel_state.rename.as_ref().map(|edit| edit.input.text()),
        Some("draft name"),
        "the half-typed name stays"
    );
    let open_row = project
        .files
        .iter()
        .find(|file| file.path == open.to_string_lossy());
    assert!(
        open_row.is_some_and(|file| file.active),
        "the open file stays selected"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// Invariant: a rescan that finds nothing new does not touch the published
/// snapshot at all.
#[test]
fn an_unchanged_rescan_publishes_nothing() {
    let root = plain_dir("quiet");
    write_op(&root, "a.op");
    write_op(&root, "sub/b.op");
    let mut app = app_with_project(&root);
    let version = app.projects_host.scan.version();
    let before = panel(&app).clone();

    for _ in 0..3 {
        assert!(!come_back(&mut app), "nothing changed, nothing published");
    }
    assert_eq!(app.projects_host.scan.version(), version);
    assert_eq!(*panel(&app), before);
    let _ = std::fs::remove_dir_all(root);
}
