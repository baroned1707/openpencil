//! Opening an empty `.op` file on a headless `DesktopApp`: every way of
//! opening lands on the same blank, formatted, bound tab, and a file that is
//! merely broken is never touched.

use super::*;
use crate::persistence;
use op_editor_core::editor_toast::EditorToastState;
use op_editor_core::{EditorState, FileAction, ProjectRequest};
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-empty-open-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn empty_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"").expect("empty file");
    path
}

fn toast(app: &DesktopApp) -> Option<EditorToastState> {
    app.host.editor_state().editor_ui.editor_toast.clone()
}

fn is_formatted(path: &Path) -> bool {
    !op_host_services::doc_io::bytes_are_blank(&std::fs::read(path).expect("read"))
}

/// What every entry point must leave behind for `path`.
fn assert_opened_as_blank(app: &DesktopApp, path: &Path, seam: &&str) {
    assert_eq!(
        app.documents.len(),
        1,
        "{seam}: the blank launch tab made room"
    );
    assert_eq!(app.current_path.as_deref(), Some(path), "{seam}: bound");
    let state = app.host.editor_state();
    assert!(!state.is_dirty(), "{seam}: clean after a successful write");
    assert_eq!(
        state.active_children().len(),
        1,
        "{seam}: the starter frame"
    );
    assert_eq!(
        state.editor_ui.file_name_display.as_deref(),
        path.file_name().and_then(|name| name.to_str()),
        "{seam}: titled by its file"
    );
    assert!(is_formatted(path), "{seam}: formatted on disk");
    // Restoring a session reopens files without reordering the recent list.
    if *seam != "session restore" {
        assert!(
            state
                .editor_ui
                .recent_files
                .iter()
                .any(|recent| Path::new(&recent.path) == path),
            "{seam}: recorded as recent"
        );
    }
    let notice = toast(app).unwrap_or_else(|| panic!("{seam}: a notice"));
    assert_eq!(notice.i18n_key, "documents.emptyFileInitialized", "{seam}");
    assert_eq!(
        notice.args,
        vec![(
            "name".to_string(),
            path.file_name().unwrap().to_string_lossy().into_owned()
        )],
        "{seam}"
    );
}

#[test]
fn empty_file_via_every_desktop_entry_point() {
    let dir = temp_dir("entry-points");
    type Seam = (&'static str, fn(&mut DesktopApp, &Path));
    let seams: [Seam; 6] = [
        // The native picker is not headless; Cmd+O hands its path to this call.
        ("menu open", |app, path| {
            assert!(app.open_document_tab(path.to_path_buf()));
        }),
        ("open recent", |app, path| {
            app.host
                .editor_state_mut()
                .editor_ui
                .touch_recent_file(path.to_string_lossy().into_owned(), 1);
            app.run_file_action(FileAction::OpenRecent(0));
        }),
        ("forward / second launch", |app, path| {
            app.forwarded_files
                .lock()
                .unwrap()
                .push_back(path.to_path_buf());
            assert!(app.drain_forwarded_files());
        }),
        ("drop", |app, path| app.on_dropped_file(path.to_path_buf())),
        ("projects dock", |app, path| {
            app.host
                .editor_state_mut()
                .editor_ui
                .projects_panel
                .pending_request = Some(ProjectRequest::OpenFile(
                path.to_string_lossy().into_owned(),
            ));
            app.pump_projects();
        }),
        ("session restore", |app, path| {
            app.pending_session_restore = vec![op_editor_core::SessionDocument {
                path: path.to_string_lossy().into_owned(),
                active: true,
            }];
            app.restore_session_documents();
        }),
    ];

    for (index, (seam, open)) in seams.iter().enumerate() {
        let path = empty_file(&dir, &format!("blank-{index}.op"));
        let mut app = DesktopApp::new(None);

        open(&mut app, &path);

        assert_opened_as_blank(&app, &path, seam);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn second_open_of_an_initialized_file_has_no_notice() {
    let dir = temp_dir("second-open");
    let path = empty_file(&dir, "blank.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(path.clone()));
    let formatted = std::fs::read(&path).unwrap();
    app.host.editor_state_mut().editor_ui.dismiss_toast();
    let mut other = DesktopApp::new(None);

    assert!(other.open_document_tab(path.clone()));

    assert!(
        toast(&other).is_none(),
        "loaded normally, nothing to announce"
    );
    assert!(!other.host.editor_state().is_dirty());
    assert_eq!(std::fs::read(&path).unwrap(), formatted, "not rewritten");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn already_open_empty_path_switches_instead_of_initializing_twice() {
    let dir = temp_dir("already-open");
    let path = empty_file(&dir, "blank.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(path.clone()));
    let other = {
        let other = dir.join("other.op");
        op_host_services::doc_io::save_to_path(&EditorState::starter(), &other).unwrap();
        other
    };
    assert!(app.open_document_tab(other));
    assert_eq!(app.documents.len(), 2);
    // Something truncates the open file behind the editor's back.
    std::fs::write(&path, b"").unwrap();
    app.host.editor_state_mut().editor_ui.dismiss_toast();

    assert!(app.open_document_tab(path.clone()), "switches to the tab");

    assert_eq!(app.documents.len(), 2, "no second tab for the path");
    assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"",
        "the switch wrote nothing: the guard wins before any initialization"
    );
    assert!(toast(&app).is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn read_only_empty_file_opens_in_memory_dirty_with_a_toast() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("read-only");
    let path = empty_file(&dir, "locked.op");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let mut app = DesktopApp::new(None);

    assert!(app.open_document_tab(path.clone()), "still opens");

    assert_eq!(app.current_path.as_deref(), Some(path.as_path()), "bound");
    assert!(
        app.host.editor_state().is_dirty(),
        "unsaved: Save can retry"
    );
    assert_eq!(app.host.editor_state().active_children().len(), 1);
    let notice = toast(&app).expect("a notice");
    assert_eq!(notice.i18n_key, "documents.emptyFileNotWritable");
    assert_eq!(std::fs::read(&path).unwrap(), b"", "left as it was");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn broken_and_non_op_files_are_never_touched_by_the_open() {
    let dir = temp_dir("untouched");
    let mut app = DesktopApp::new(None);
    let samples: [(&str, &[u8]); 6] = [
        ("garbage.op", b"not a document"),
        ("partial.op", b"{}"),
        ("brace.op", b"{"),
        ("empty.pen", b""),
        ("empty.txt", b""),
        ("empty.fig", b""),
    ];
    for (name, content) in samples {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();

        // `load_document` is what the open path calls; its `Err` is what the
        // error dialog is shown for (the dialog itself is modal and native).
        let result = persistence::load_document(&mut app.host, &path, false, true);

        assert!(result.is_err(), "{name} still reports the load error");
        assert_eq!(std::fs::read(&path).unwrap(), content, "{name} untouched");
    }
    assert_eq!(app.documents.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_replacement_that_only_rereads_a_file_never_formats_it() {
    let dir = temp_dir("reread");
    let path = empty_file(&dir, "blank.op");
    let mut app = DesktopApp::new(None);

    let result = persistence::load_document(&mut app.host, &path, true, false);

    assert!(result.is_err(), "the plain loader keeps rejecting it");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"",
        "no write without opt-in"
    );
    let _ = std::fs::remove_dir_all(dir);
}
