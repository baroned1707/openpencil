//! Reveal-in-dock host behaviour: the breadcrumb's request opens the dock,
//! unfolds what hides the row, and does nothing for a file the dock lacks.

use crate::projects_host::tests::{panel, settle, temp_dir, write_design};
use crate::DesktopApp;
use op_editor_core::ProjectRequest;
use std::path::Path;

fn request(app: &mut DesktopApp, request: ProjectRequest) {
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(request);
    assert!(app.pump_projects());
    settle(app);
}

fn fold_everything(app: &mut DesktopApp, root: &Path) {
    let root = root.to_string_lossy().into_owned();
    request(app, ProjectRequest::ToggleProject(root.clone()));
    request(
        app,
        ProjectRequest::ToggleFolder(root, "designs".to_string()),
    );
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .dock_open = false;
}

#[test]
fn reveal_expands_a_folded_project_and_folder_and_opens_the_dock() {
    let root = temp_dir("reveal");
    let a = write_design(&root, "a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a.clone()));
    settle(&mut app);
    fold_everything(&mut app, &root);
    assert!(!panel(&app)[0].expanded, "fixture: the project is folded");
    assert_eq!(panel(&app)[0].collapsed_folders, ["designs"]);

    request(
        &mut app,
        ProjectRequest::RevealFile(a.to_string_lossy().into_owned()),
    );

    let project = &panel(&app)[0];
    assert!(project.expanded, "the project is unfolded");
    assert!(
        project.collapsed_folders.is_empty(),
        "the folder is unfolded"
    );
    assert!(
        app.host.editor_state().editor_ui.projects_panel.is_open(),
        "the folded dock is open again"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reveal_leaves_unrelated_folds_and_the_dock_alone_when_the_row_is_gone() {
    let root = temp_dir("reveal-gone");
    let a = write_design(&root, "a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a));
    settle(&mut app);
    fold_everything(&mut app, &root);

    let gone = root.join("designs").join("deleted.op");
    request(
        &mut app,
        ProjectRequest::RevealFile(gone.to_string_lossy().into_owned()),
    );

    assert!(!panel(&app)[0].expanded, "no row, so no unfolding");
    assert!(
        !app.host.editor_state().editor_ui.projects_panel.is_open(),
        "and the dock stays as the user left it"
    );
    assert!(app.projects_host.pending_reveal.is_none());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reveal_of_a_file_already_in_view_changes_nothing_but_still_succeeds() {
    let root = temp_dir("reveal-visible");
    let a = write_design(&root, "a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a.clone()));
    settle(&mut app);
    let before = panel(&app).clone();
    request(
        &mut app,
        ProjectRequest::RevealFile(a.to_string_lossy().into_owned()),
    );
    assert_eq!(*panel(&app), before);
    assert!(app.host.editor_state().editor_ui.projects_panel.is_open());
    let _ = std::fs::remove_dir_all(root);
}
