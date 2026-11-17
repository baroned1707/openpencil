//! "New design here": where the file goes, what it is called, that the owner can
//! name it at once, and every way the request can be refused. Every case builds
//! its own folders under the OS temp dir.

use super::super::tests::{panel, settle, toast_key};
use super::super::*;
use super::*;
use op_host_services::doc_io::save_to_path;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn plain_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-create-here-{tag}-{}-{}",
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

fn app_with_project(root: &Path) -> DesktopApp {
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(root));
    settle(&mut app);
    app
}

fn here(app: &mut DesktopApp, root: &Path, folder: &str) {
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::NewDesignHere(
        root.to_string_lossy().into_owned(),
        folder.to_string(),
    ));
    app.pump_projects();
    settle(app);
}

fn renaming(app: &DesktopApp) -> Option<(String, String)> {
    app.host
        .editor_state()
        .editor_ui
        .projects_panel
        .rename
        .as_ref()
        .map(|edit| (edit.path.clone(), edit.input.text().to_string()))
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn create_here_makes_unique_name_and_starts_rename() {
    let root = plain_dir("unique");
    write_op(&root, "screens/Untitled.op");
    write_op(&root, "screens/Untitled 2.op");
    let mut app = app_with_project(&root);
    let tabs = app.documents.len();
    // The folder is folded: the new row must unfold it to be seen.
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::ToggleFolder(
        root.to_string_lossy().into_owned(),
        "screens".to_string(),
    ));
    app.pump_projects();
    settle(&mut app);
    assert!(panel(&app)[0].is_folder_collapsed("screens"));

    here(&mut app, &root, "screens");

    let created = root.join("screens/Untitled 3.op");
    assert!(
        created.is_file(),
        "the first free name: {:?}",
        names(&root.join("screens"))
    );
    assert_eq!(toast_key(&app), None);
    assert!(
        panel(&app)[0]
            .files
            .iter()
            .any(|file| file.path == created.to_string_lossy()),
        "the row is listed at once"
    );
    assert!(
        !panel(&app)[0].is_folder_collapsed("screens"),
        "the folder unfolded"
    );
    assert_eq!(
        renaming(&app),
        Some((
            created.to_string_lossy().into_owned(),
            "Untitled 3".to_string()
        )),
        "the owner is naming it, stem only"
    );
    let edit = app
        .host
        .editor_state()
        .editor_ui
        .projects_panel
        .rename
        .as_ref()
        .unwrap();
    assert_eq!(
        edit.input.highlight_range(),
        Some((0, 10)),
        "the stem is selected"
    );
    assert_eq!(app.documents.len(), tabs, "no tab was opened");
    // It is a real design: the starter content, readable as one.
    assert!(std::fs::metadata(&created).unwrap().len() > 0);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_header_creates_in_the_project_root() {
    let root = plain_dir("root");
    write_op(&root, "a.op");
    let mut app = app_with_project(&root);
    here(&mut app, &root, "");
    assert!(root.join("Untitled.op").is_file());
    assert_eq!(
        renaming(&app).map(|(path, _)| path),
        Some(root.join("Untitled.op").to_string_lossy().into_owned())
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_in_vanished_folder_fails_with_toast() {
    let root = plain_dir("vanished");
    write_op(&root, "gone/a.op");
    write_op(&root, "kept/b.op");
    let mut app = app_with_project(&root);
    std::fs::remove_dir_all(root.join("gone")).unwrap();

    here(&mut app, &root, "gone");

    assert_eq!(toast_key(&app).as_deref(), Some("projects.newDesignFailed"));
    assert!(
        !root.join("gone").exists(),
        "the folder is not brought back"
    );
    assert_eq!(renaming(&app), None);
    assert_eq!(names(&root), ["kept"], "nothing was created anywhere");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn create_outside_root_is_refused() {
    let root = plain_dir("outside");
    write_op(&root, "a.op");
    let other = plain_dir("outside-other");
    let mut app = app_with_project(&root);

    // A project the dock does not show.
    here(&mut app, &other, "");
    assert!(names(&other).is_empty(), "nothing was written next door");
    assert_eq!(toast_key(&app).as_deref(), Some("projects.newDesignFailed"));

    // Folders that climb out, are absolute, or are hidden.
    for folder in ["..", "../outside", "/tmp", ".git", "sub/../.."] {
        app.host.editor_state_mut().editor_ui.dismiss_toast();
        here(&mut app, &root, folder);
        assert_eq!(
            toast_key(&app).as_deref(),
            Some("projects.newDesignFailed"),
            "{folder:?}"
        );
    }
    assert_eq!(names(&root), ["a.op"], "the project is untouched");
    assert_eq!(renaming(&app), None);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(other);
}

#[cfg(unix)]
#[test]
fn a_link_on_the_way_is_refused() {
    let root = plain_dir("link");
    write_op(&root, "real/a.op");
    let elsewhere = plain_dir("link-out");
    std::os::unix::fs::symlink(&elsewhere, root.join("hop")).unwrap();
    std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
    let roots = vec![root.clone()];
    for folder in ["hop", "alias"] {
        assert!(
            plan_folder(&roots, true, &root, folder).is_err(),
            "{folder}: a link is never a folder to write into"
        );
    }
    assert!(names(&elsewhere).is_empty());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(elsewhere);
}

#[test]
fn an_inferred_project_only_takes_new_designs_in_its_designs_folder() {
    let root = plain_dir("inferred");
    std::fs::create_dir_all(root.join("designs")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let roots = vec![root.clone()];
    assert!(plan_folder(&roots, false, &root, "designs").is_ok());
    assert_eq!(
        plan_folder(&roots, false, &root, "src"),
        Err(CreateHereRefusal::NotListed),
        "it would never be listed"
    );
    assert_eq!(
        plan_folder(&roots, false, &root, ""),
        Err(CreateHereRefusal::NotListed)
    );
    assert!(
        plan_folder(&roots, true, &root, "src").is_ok(),
        "an opened folder lists it"
    );
    assert_eq!(
        plan_folder(&roots, true, &root, "node_modules"),
        Err(CreateHereRefusal::NotListed),
        "a folder the walk skips"
    );
    let _ = std::fs::remove_dir_all(root);
}
