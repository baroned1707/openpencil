//! Opening a folder as a project: what the dock lists, which folders are
//! refused, how documents group under an opened folder, what is saved and
//! restored, and that a closed project leaves its tabs alone. Every case builds
//! its own folders under the OS temp dir; the folder picker is injected.

use super::tests::{panel, settle, toast_key, write_design};
use super::*;
use op_host_services::doc_io::save_to_path;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A canonical folder with NO project marker in it.
fn plain_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-open-project-{tag}-{}-{}",
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

fn explicit(app: &DesktopApp) -> Vec<PathBuf> {
    app.documents.explicit_roots().snapshot()
}

fn saved_projects(app: &DesktopApp) -> Vec<String> {
    app.host.editor_state().editor_ui.open_projects.clone()
}

fn tab_projects(app: &mut DesktopApp) -> Vec<String> {
    app.sync_active_slot();
    app.documents
        .tab_views(false, "")
        .into_iter()
        .map(|tab| tab.project)
        .collect()
}

fn request(app: &mut DesktopApp, request: ProjectRequest) {
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(request);
    app.pump_projects();
}

#[test]
fn open_project_lists_every_op_file_recursively_sorted() {
    let root = plain_dir("list");
    for relative in [
        "z.op",
        "designs/home.op",
        "screens/auth/login.op",
        "screens/auth/deep/signup.op",
        "screens/cart.op",
        "a.op",
    ] {
        write_op(&root, relative);
    }
    std::fs::write(root.join("notes.txt"), "x").unwrap();
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);

    let projects = panel(&app);
    assert_eq!(projects.len(), 1);
    let project = &projects[0];
    assert_eq!(project.root, root.to_string_lossy());
    assert!(project.closable);
    assert_eq!(project.scan, ProjectScanState::Idle);
    let listed: Vec<(String, String)> = project
        .files
        .iter()
        .map(|file| (file.folder.clone(), file.label.clone()))
        .collect();
    assert_eq!(
        listed,
        [
            ("", "a.op"),
            ("", "z.op"),
            ("designs", "home.op"),
            ("screens", "cart.op"),
            ("screens/auth", "login.op"),
            ("screens/auth/deep", "signup.op"),
        ]
        .map(|(folder, name)| (folder.to_string(), name.to_string())),
        "grouped by folder, the root's own files first"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn project_without_markers_is_still_a_project() {
    let root = plain_dir("markerless");
    write_op(&root, "one.op");
    assert!(!root.join(".git").exists() && !root.join("package.json").exists());
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    assert_eq!(panel(&app).len(), 1);
    assert_eq!(panel(&app)[0].files.len(), 1);
    assert_eq!(saved_projects(&app), [root.to_string_lossy().into_owned()]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_folder_with_no_op_files_is_still_opened_and_offers_a_new_design() {
    let root = plain_dir("empty");
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    assert_eq!(panel(&app).len(), 1);
    assert!(panel(&app)[0].files.is_empty());
    assert_eq!(panel(&app)[0].scan, ProjectScanState::Idle);
    request(
        &mut app,
        ProjectRequest::NewDesignIn(root.to_string_lossy().into_owned()),
    );
    assert!(root.join("designs/untitled.op").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn unsafe_roots_are_refused_without_side_effects() {
    let tree = plain_dir("unsafe");
    let home = tree.join("home/me");
    let system = tree.join("System");
    std::fs::create_dir_all(system.join("Library")).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let mut app = DesktopApp::new(None);
    app.projects_host.home_override = Some(Some(home.clone()));
    app.projects_host.system_dirs = op_host_services::project_root::SystemDirs {
        exact: Vec::new(),
        trees: vec![system.clone()],
    };
    let saved_before = saved_projects(&app);
    for refused in [
        PathBuf::from("/"),
        home.clone(),
        tree.join("home"),
        system.join("Library"),
    ] {
        assert!(
            !app.open_project(&refused),
            "{} must be refused",
            refused.display()
        );
        assert_eq!(toast_key(&app).as_deref(), Some("project.unsafeRoot"));
        app.host.editor_state_mut().editor_ui.editor_toast = None;
    }
    app.publish_projects();
    assert!(explicit(&app).is_empty(), "nothing was registered");
    assert!(panel(&app).is_empty(), "nothing is listed");
    assert!(app.projects_host.known.is_empty());
    assert!(app.projects_host.awaiting.is_empty(), "no scan was queued");
    assert_eq!(saved_projects(&app), saved_before, "nothing was saved");

    // A missing folder is unreadable, not unsafe.
    assert!(!app.open_project(&tree.join("missing")));
    assert_eq!(toast_key(&app).as_deref(), Some("project.unreadable"));
    let _ = std::fs::remove_dir_all(tree);
}

#[test]
fn document_inside_an_explicit_root_groups_under_it_in_tab_and_dock() {
    let root = plain_dir("group");
    // Inference alone would call `pkg` the project (it has a marker).
    std::fs::create_dir_all(root.join("pkg")).unwrap();
    std::fs::write(root.join("pkg/package.json"), "{}").unwrap();
    let inside = write_op(&root, "pkg/designs/a.op");
    let outside_root = plain_dir("group-other");
    std::fs::write(outside_root.join("package.json"), "{}").unwrap();
    let outside = write_design(&outside_root, "b.op");

    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(inside.clone()));
    assert!(app.open_document_tab(outside));
    assert_eq!(
        tab_projects(&mut app),
        [
            "pkg",
            outside_root.file_name().unwrap().to_string_lossy().as_ref()
        ],
        "inferred before the folder is opened"
    );
    assert!(app.open_project(&root));
    settle(&mut app);

    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    let tabs = tab_projects(&mut app);
    assert_eq!(tabs[0], name, "the tab regroups under the opened folder");
    assert_ne!(
        tabs[1], name,
        "a document outside every opened folder keeps inference"
    );
    let roots: Vec<&str> = panel(&app).iter().map(|p| p.root.as_str()).collect();
    assert!(roots.contains(&root.to_string_lossy().as_ref()));
    assert!(
        !roots.iter().any(|candidate| candidate.ends_with("/pkg")),
        "the inferred `pkg` project is part of the opened folder now: {roots:?}"
    );
    let project = panel(&app)
        .iter()
        .find(|p| p.root == root.to_string_lossy())
        .unwrap();
    assert!(project
        .files
        .iter()
        .any(|file| file.open_id.is_some() && file.folder == "pkg/designs"));
    // The agent starts in the opened folder, not in the nearer marker's folder.
    assert_eq!(
        app.documents.explicit_roots().root_for(&inside).as_deref(),
        Some(root.as_path())
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside_root);
}

#[test]
fn the_agent_working_directory_inside_an_explicit_root_is_that_root() {
    let root = plain_dir("cwd");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/package.json"), "{}").unwrap();
    let file = write_op(&root, "sub/a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(file));
    assert_eq!(
        app.launch_context().project_cwd,
        Some(root.join("sub")),
        "inferred before the folder is opened"
    );
    assert!(app.open_project(&root));
    assert_eq!(app.launch_context().project_cwd, Some(root.clone()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn deepest_explicit_root_wins() {
    let outer = plain_dir("deep");
    let inner = outer.join("apps/web");
    let in_outer = write_op(&outer, "root.op");
    let in_inner = write_op(&inner, "screens/home.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(in_inner.clone()));
    assert!(app.open_project(&outer));
    assert!(app.open_project(&inner));
    settle(&mut app);

    assert_eq!(
        tab_projects(&mut app),
        ["web"],
        "the deeper folder names the tab"
    );
    assert_eq!(
        app.documents.explicit_roots().root_for(&in_inner),
        Some(inner.clone())
    );
    assert_eq!(
        app.documents.explicit_roots().root_for(&in_outer),
        Some(outer.clone())
    );
    let by_root = |root: &Path| {
        panel(&app)
            .iter()
            .find(|project| project.root == root.to_string_lossy())
            .unwrap_or_else(|| panic!("{} is listed", root.display()))
    };
    let outer_files: Vec<&str> = by_root(&outer)
        .files
        .iter()
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(
        outer_files,
        ["root.op"],
        "the deeper folder's files are not repeated"
    );
    let inner_files: Vec<&str> = by_root(&inner)
        .files
        .iter()
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(inner_files, ["home.op"]);
    let _ = std::fs::remove_dir_all(outer);
}

#[test]
fn open_projects_persist_and_restore_before_documents() {
    let root = plain_dir("persist");
    let file = write_op(&root, "designs/a.op");
    let mut first = DesktopApp::new(None);
    assert!(first.open_project(&root));
    assert_eq!(
        saved_projects(&first),
        [root.to_string_lossy().into_owned()]
    );

    // A fresh launch restores the folders first, then the documents, so the
    // tab groups under the folder the moment it exists.
    let mut second = DesktopApp::new(None);
    second.projects_host = ProjectsHostState::restoring(saved_projects(&first));
    second.pending_session_restore = vec![op_editor_core::SessionDocument {
        path: file.to_string_lossy().into_owned(),
        active: true,
    }];
    second.restore_open_projects();
    assert_eq!(
        explicit(&second),
        std::slice::from_ref(&root),
        "folders are back before any tab"
    );
    assert!(second.documents.len() == 1 && second.current_path.is_none());
    second.restore_session_documents();
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(tab_projects(&mut second), [name]);
    settle(&mut second);
    assert!(panel(&second)[0].closable);
    assert_eq!(
        saved_projects(&second),
        [root.to_string_lossy().into_owned()]
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn restore_skips_missing_folders_and_prunes_them() {
    let kept = plain_dir("restore-kept");
    let gone = plain_dir("restore-gone");
    std::fs::remove_dir_all(&gone).unwrap();
    let mut app = DesktopApp::new(None);
    app.projects_host = ProjectsHostState::restoring(vec![
        gone.to_string_lossy().into_owned(),
        "/".to_string(),
        kept.to_string_lossy().into_owned(),
        "   ".to_string(),
    ]);
    app.restore_open_projects();
    assert_eq!(
        explicit(&app),
        std::slice::from_ref(&kept),
        "only the readable, safe folder"
    );
    assert_eq!(
        saved_projects(&app),
        [kept.to_string_lossy().into_owned()],
        "the others are pruned from the saved list"
    );
    assert_eq!(toast_key(&app), None, "pruning is silent");
    let _ = std::fs::remove_dir_all(kept);
}

#[test]
fn close_project_keeps_its_open_tabs() {
    let root = plain_dir("close");
    let file = write_op(&root, "designs/a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(file.clone()));
    assert!(app.open_project(&root));
    settle(&mut app);
    assert!(panel(&app)[0].closable);

    request(
        &mut app,
        ProjectRequest::CloseProject(root.to_string_lossy().into_owned()),
    );
    settle(&mut app);
    assert_eq!(app.documents.len(), 1, "the tab stays open");
    assert_eq!(app.current_path.as_deref(), Some(file.as_path()));
    assert!(explicit(&app).is_empty());
    assert!(
        saved_projects(&app).is_empty(),
        "the closed folder is not restored"
    );
    assert!(
        panel(&app).iter().all(|project| !project.closable),
        "what is left is inferred from the open document"
    );
    assert_eq!(panel(&app).len(), 1);
    // Closing something that is not an opened project does nothing.
    request(
        &mut app,
        ProjectRequest::CloseProject(root.to_string_lossy().into_owned()),
    );
    assert_eq!(app.documents.len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dropping_a_folder_opens_it_as_a_project_and_a_file_still_opens_as_a_tab() {
    let root = plain_dir("drop");
    let file = write_op(&root, "designs/a.op");
    let mut app = DesktopApp::new(None);
    app.on_dropped_file(root.clone());
    assert_eq!(explicit(&app), std::slice::from_ref(&root));
    assert_eq!(app.documents.len(), 1, "a folder opens no tab");

    // The only tab is a pristine untitled one, which a file opens into.
    app.on_dropped_file(file.clone());
    assert_eq!(app.current_path.as_deref(), Some(file.as_path()));
    let other = write_op(&root, "designs/b.op");
    app.on_dropped_file(other.clone());
    assert_eq!(app.documents.len(), 2, "a dropped file is still a tab");
    assert_eq!(app.current_path.as_deref(), Some(other.as_path()));
    settle(&mut app);
    assert_eq!(panel(&app).len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn picker_cancel_changes_nothing() {
    let root = plain_dir("picker");
    let mut app = DesktopApp::new(None);
    let asked = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = std::rc::Rc::clone(&asked);
    app.projects_host.picker = Box::new(move |title| {
        counter.set(counter.get() + 1);
        assert_eq!(
            title, "Open Project",
            "the dialog is titled like the menu item"
        );
        None
    });
    request(&mut app, ProjectRequest::OpenProjectPicker);
    assert_eq!(asked.get(), 1);
    assert!(explicit(&app).is_empty());
    assert!(panel(&app).is_empty());
    assert!(saved_projects(&app).is_empty());
    assert_eq!(toast_key(&app), None);

    let chosen = root.clone();
    app.projects_host.picker = Box::new(move |_| Some(chosen.clone()));
    request(&mut app, ProjectRequest::OpenProjectPicker);
    assert_eq!(explicit(&app), std::slice::from_ref(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_design_in_an_explicit_root_creates_under_designs() {
    let root = plain_dir("new-design");
    write_op(&root, "screens/a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    request(
        &mut app,
        ProjectRequest::NewDesignIn(root.to_string_lossy().into_owned()),
    );
    let created = root.join("designs/untitled.op");
    assert!(created.is_file());
    assert_eq!(app.current_path.as_deref(), Some(created.as_path()));
    settle(&mut app);
    let project = &panel(&app)[0];
    assert!(project.files.iter().any(|file| file.folder == "designs"
        && file.label == "untitled.op"
        && file.open_id.is_some()));
    // Closing its tab must not make the new file vanish from the list.
    let index = app.documents.index_of_path(&created).expect("its tab");
    assert!(app.close_document_tab(index));
    settle(&mut app);
    let listed = panel(&app)[0]
        .files
        .iter()
        .find(|file| file.label == "untitled.op")
        .cloned()
        .expect("still listed after its tab closed");
    assert_eq!(listed.open_id, None);
    let _ = std::fs::remove_dir_all(root);
}

/// A scheduler whose walk job counts its runs.
fn counting_tree_state() -> (ProjectsHostState, Arc<std::sync::atomic::AtomicUsize>) {
    let walks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&walks);
    let tree_job: scheduler::ScanJob = Arc::new(move |roots| {
        counted.fetch_add(1, Ordering::SeqCst);
        roots
            .iter()
            .map(|root| {
                (
                    root.clone(),
                    scan_tree::scan_project_tree(root, &scan_tree::TreeLimits::default()),
                )
            })
            .collect()
    });
    let designs_job: scheduler::ScanJob = Arc::new(|roots| {
        roots
            .iter()
            .map(|root| (root.clone(), scan::scan_design_dir(root)))
            .collect()
    });
    // Periodic rescans pushed out of reach: only requests start a scan.
    let scan = ScanScheduler::with_timing(designs_job, Duration::ZERO, scheduler::SLOW_AFTER)
        .with_tree_job(tree_job);
    let cache = op_host_services::project_root::shared_cache();
    (ProjectsHostState::with_parts(cache, scan), walks)
}

#[test]
fn refresh_rescans_without_a_frame_cost_when_idle() {
    let root = plain_dir("refresh");
    write_op(&root, "a.op");
    let (state, walks) = counting_tree_state();
    let mut app = DesktopApp::new(None);
    app.projects_host = state;
    assert!(app.open_project(&root));
    settle(&mut app);
    assert_eq!(walks.load(Ordering::SeqCst), 1, "one walk when opened");
    assert_eq!(panel(&app)[0].scan, ProjectScanState::Idle);

    // Idle frames (even with the periodic tick due every time) never re-walk
    // an opened folder and republish nothing.
    for _ in 0..50 {
        assert!(!app.publish_projects());
    }
    assert_eq!(walks.load(Ordering::SeqCst), 1);

    // A file appears behind the app's back: only a refresh finds it.
    write_op(&root, "b.op");
    assert_eq!(panel(&app)[0].files.len(), 1);
    request(
        &mut app,
        ProjectRequest::RefreshProject(root.to_string_lossy().into_owned()),
    );
    assert_eq!(
        panel(&app)[0].scan,
        ProjectScanState::Scanning,
        "while it runs"
    );
    settle(&mut app);
    assert_eq!(walks.load(Ordering::SeqCst), 2, "exactly one more walk");
    assert_eq!(panel(&app)[0].files.len(), 2);
    assert_eq!(panel(&app)[0].scan, ProjectScanState::Idle);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn folders_fold_and_unfold_through_requests() {
    let root = plain_dir("fold");
    write_op(&root, "screens/a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    let key = root.to_string_lossy().into_owned();
    request(
        &mut app,
        ProjectRequest::ToggleFolder(key.clone(), "screens".into()),
    );
    assert_eq!(panel(&app)[0].collapsed_folders, ["screens"]);
    assert_eq!(panel(&app)[0].files.len(), 1, "the files stay in the view");
    request(
        &mut app,
        ProjectRequest::ToggleFolder(key, "screens".into()),
    );
    assert!(panel(&app)[0].collapsed_folders.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_opened_folder_reopened_is_scanned_again_not_duplicated() {
    let root = plain_dir("again");
    write_op(&root, "a.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_project(&root));
    settle(&mut app);
    write_op(&root, "b.op");
    assert!(app.open_project(&root));
    settle(&mut app);
    assert_eq!(explicit(&app).len(), 1);
    assert_eq!(panel(&app).len(), 1);
    assert_eq!(panel(&app)[0].files.len(), 2);
    assert_eq!(saved_projects(&app).len(), 1);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_empty_dock_opens_the_picker_from_its_strip() {
    let root = plain_dir("strip");
    let mut app = DesktopApp::new(None);
    {
        let state = app.host.editor_state();
        assert!(state.editor_ui.projects_dock_available());
        assert!(
            !state.editor_ui.projects_panel.is_open(),
            "starts as a strip"
        );
    }
    let chosen = root.clone();
    app.projects_host.picker = Box::new(move |_| Some(chosen.clone()));
    // What a click on the strip's folder-plus button records.
    request(&mut app, ProjectRequest::OpenProjectPicker);
    settle(&mut app);
    assert_eq!(panel(&app).len(), 1);
    assert!(
        app.host.editor_state().editor_ui.projects_panel.is_open(),
        "the first project unfolds the dock"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_live_mcp_server_learns_the_opened_folders_as_allowed_roots() {
    let root = plain_dir("mcp");
    let mut app = DesktopApp::new(None);
    let Ok(server) = op_host_services::mcp_live::McpLiveServer::start(0) else {
        return; // No loopback in this sandbox: the policy itself is tested in op-host-services.
    };
    app.mcp_server = Some(server);
    app.sync_mcp_project_roots();
    assert!(app.mcp_server.as_ref().unwrap().extra_roots().is_empty());
    assert!(app.open_project(&root));
    app.sync_mcp_project_roots();
    assert_eq!(
        app.mcp_server.as_ref().unwrap().extra_roots(),
        std::slice::from_ref(&root)
    );
    app.close_project(&root);
    app.sync_mcp_project_roots();
    assert!(app.mcp_server.as_ref().unwrap().extra_roots().is_empty());
    if let Some(mut server) = app.mcp_server.take() {
        server.stop();
    }
    let _ = std::fs::remove_dir_all(root);
}
