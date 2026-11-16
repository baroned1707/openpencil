//! Projects tab host wiring: tree building, disk listing and requests.

use super::*;
use op_host_services::doc_io::save_to_path;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-projects-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    // A marker so the folder is a project root of its own.
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    dir.canonicalize().unwrap()
}

fn write_design(root: &Path, name: &str) -> PathBuf {
    let designs = root.join("designs");
    std::fs::create_dir_all(&designs).unwrap();
    let path = designs.join(name);
    save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
    path
}

fn panel(app: &DesktopApp) -> &Vec<ProjectView> {
    &app.host.editor_state().editor_ui.projects_panel.projects
}

#[test]
fn lists_design_files_on_disk_sorted_and_only_op_files() {
    let root = temp_dir("list");
    write_design(&root, "b.op");
    write_design(&root, "a.op");
    std::fs::write(root.join("designs").join("notes.txt"), "x").unwrap();
    let names: Vec<String> = list_design_files(&root)
        .iter()
        .map(|p| file_label(p))
        .collect();
    assert_eq!(names, ["a.op", "b.op"]);
    assert!(list_design_files(&root.join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn builder_marks_open_files_and_keeps_unopened_ones_listed() {
    let root = PathBuf::from("/p/shop");
    let a = root.join("designs/a.op");
    let b = root.join("designs/b.op");
    let roots: BTreeSet<PathBuf> = [root.clone()].into();
    let listing = BTreeMap::from([(root.clone(), vec![a.clone(), b.clone()])]);
    let open = [OpenDoc {
        id: DocumentId(7),
        path: Some(a.clone()),
        dirty: true,
        status: DocumentTabStatus::NeedsReview,
        active: true,
    }];
    let root_for = |_: &Path| Some(PathBuf::from("/p/shop"));
    let projects = build_projects(&roots, &listing, &open, &HashSet::new(), &root_for);
    assert_eq!(projects.len(), 1);
    let files = &projects[0].files;
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].open_id, Some(DocumentId(7)));
    assert!(files[0].dirty && files[0].active);
    assert_eq!(files[0].status, DocumentTabStatus::NeedsReview);
    assert_eq!(files[1].open_id, None, "b is on disk but not open");
    assert_eq!(projects[0].needs_review_count(), 1);
}

#[test]
fn an_open_file_outside_designs_still_shows_in_its_project() {
    let root = PathBuf::from("/p/shop");
    let loose = root.join("sketch.op");
    let roots: BTreeSet<PathBuf> = [root.clone()].into();
    let open = [OpenDoc {
        id: DocumentId(1),
        path: Some(loose.clone()),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
    }];
    let root_for = |_: &Path| Some(PathBuf::from("/p/shop"));
    let projects = build_projects(&roots, &BTreeMap::new(), &open, &HashSet::new(), &root_for);
    assert_eq!(projects[0].files.len(), 1);
    assert_eq!(projects[0].files[0].label, "sketch.op");
}

#[test]
fn collapsed_flag_is_carried_into_the_row() {
    let root = PathBuf::from("/p/shop");
    let roots: BTreeSet<PathBuf> = [root.clone()].into();
    let collapsed: HashSet<PathBuf> = [root].into();
    let none = |_: &Path| None;
    let projects = build_projects(&roots, &BTreeMap::new(), &[], &collapsed, &none);
    assert!(!projects[0].expanded);
}

#[test]
fn next_untitled_design_never_reuses_a_name() {
    let root = temp_dir("untitled");
    assert!(next_untitled_design(&root).ends_with("designs/untitled.op"));
    write_design(&root, "untitled.op");
    assert!(next_untitled_design(&root).ends_with("designs/untitled-2.op"));
    write_design(&root, "untitled-2.op");
    assert!(next_untitled_design(&root).ends_with("designs/untitled-3.op"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opening_a_file_publishes_its_project_with_sibling_files() {
    let root = temp_dir("publish");
    let a = write_design(&root, "a.op");
    write_design(&root, "b.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a.clone()));
    assert!(app.publish_projects());
    let projects = panel(&app);
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].files.len(), 2, "the unopened sibling is listed");
    let open: Vec<_> = projects[0]
        .files
        .iter()
        .filter(|f| f.open_id.is_some())
        .collect();
    assert_eq!(open.len(), 1);
    assert!(open[0].active);
    assert!(
        !app.publish_projects(),
        "nothing changed, nothing republished"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn project_requests_open_switch_toggle_and_create() {
    let root = temp_dir("requests");
    let a = write_design(&root, "a.op");
    let b = write_design(&root, "b.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a.clone()));
    app.publish_projects();

    // Open a file that is not open yet -> new tab.
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::OpenFile(b.to_string_lossy().into_owned()));
    assert!(app.pump_projects());
    assert_eq!(app.documents.len(), 2);
    assert_eq!(app.current_path.as_deref(), Some(b.as_path()));

    // Switch back to the first by id.
    let first_id = app.documents.id_at(0).unwrap();
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::SwitchTo(first_id));
    app.pump_projects();
    assert_eq!(app.current_path.as_deref(), Some(a.as_path()));

    // Fold the project.
    let key = root.to_string_lossy().into_owned();
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::ToggleProject(key.clone()));
    app.pump_projects();
    assert!(!panel(&app)[0].expanded);
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::ToggleProject(key.clone()));
    app.pump_projects();
    assert!(panel(&app)[0].expanded);

    // New design in the project: a fresh file on disk, opened as a tab.
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::NewDesignIn(key));
    app.pump_projects();
    let created = root.join("designs/untitled.op");
    assert!(created.is_file(), "the new design exists on disk");
    assert_eq!(app.current_path.as_deref(), Some(created.as_path()));
    assert_eq!(app.documents.len(), 3);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_project_stays_listed_after_its_last_tab_closes() {
    let root = temp_dir("stay");
    let a = write_design(&root, "a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a));
    app.publish_projects();
    assert!(app.close_document_tab(0));
    app.publish_projects();
    assert_eq!(panel(&app).len(), 1, "closing the tab keeps the project");
    assert_eq!(panel(&app)[0].files[0].open_id, None);
    let _ = std::fs::remove_dir_all(root);
}
