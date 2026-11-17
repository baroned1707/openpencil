//! Projects dock host wiring: tree building, disk listing and requests.

use super::*;
use op_host_services::doc_io::save_to_path;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn temp_dir(tag: &str) -> PathBuf {
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

pub(super) fn write_design(root: &Path, name: &str) -> PathBuf {
    let designs = root.join("designs");
    std::fs::create_dir_all(&designs).unwrap();
    let path = designs.join(name);
    save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
    path
}

/// Pump the frame until the scan worker has delivered and nothing is queued.
/// Returns whether any frame in between changed the published tree.
pub(super) fn settle(app: &mut DesktopApp) -> bool {
    let started = Instant::now();
    let mut changed = false;
    loop {
        changed |= app.publish_projects();
        if app.projects_host.scan.is_idle() {
            return changed;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "scan never settled"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn panel(app: &DesktopApp) -> &Vec<ProjectView> {
    &app.host.editor_state().editor_ui.projects_panel.projects
}

fn idle(_: &Path) -> ProjectScanState {
    ProjectScanState::Idle
}

/// Builder inputs for inferred projects only: nothing opened, nothing folded.
fn inputs<'a>(
    roots: &'a BTreeSet<PathBuf>,
    listing: &'a BTreeMap<PathBuf, Vec<PathBuf>>,
    open: &'a [OpenDoc],
    collapsed: &'a HashSet<PathBuf>,
) -> TreeInputs<'a> {
    static NO_FOLDERS: std::sync::OnceLock<HashSet<(PathBuf, String)>> = std::sync::OnceLock::new();
    static NO_JOBS: std::sync::OnceLock<BTreeMap<PathBuf, fig::JobStatus>> =
        std::sync::OnceLock::new();
    TreeInputs {
        roots,
        explicit: &[],
        listing,
        open,
        collapsed,
        collapsed_folders: NO_FOLDERS.get_or_init(HashSet::new),
        fig_jobs: NO_JOBS.get_or_init(BTreeMap::new),
        scan_state: &idle,
    }
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
    let projects = build_projects(&inputs(&roots, &listing, &open, &HashSet::new()), &root_for);
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
    let projects = build_projects(
        &inputs(&roots, &BTreeMap::new(), &open, &HashSet::new()),
        &root_for,
    );
    assert_eq!(projects[0].files.len(), 1);
    assert_eq!(projects[0].files[0].label, "sketch.op");
}

#[test]
fn collapsed_flag_is_carried_into_the_row() {
    let root = PathBuf::from("/p/shop");
    let roots: BTreeSet<PathBuf> = [root.clone()].into();
    let collapsed: HashSet<PathBuf> = [root].into();
    let none = |_: &Path| None;
    let projects = build_projects(&inputs(&roots, &BTreeMap::new(), &[], &collapsed), &none);
    assert!(!projects[0].expanded);
}

#[test]
fn opening_a_file_publishes_its_project_with_sibling_files() {
    let root = temp_dir("publish");
    let a = write_design(&root, "a.op");
    write_design(&root, "b.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(a.clone()));
    assert!(settle(&mut app));
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
    settle(&mut app);

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
    settle(&mut app);
    assert!(app.close_document_tab(0));
    settle(&mut app);
    assert_eq!(panel(&app).len(), 1, "closing the tab keeps the project");
    assert_eq!(panel(&app)[0].files[0].open_id, None);
    let _ = std::fs::remove_dir_all(root);
}

/// A root cache over the real resolver that counts how often it is asked, and a
/// scan scheduler whose job counts its runs. Periodic rescans are pushed out of
/// reach so only requests start a scan.
fn counting_parts() -> (
    ProjectsHostState,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    Arc<ProjectRootCache>,
) {
    let resolutions = Arc::new(AtomicUsize::new(0));
    let resolved = Arc::clone(&resolutions);
    let cache = Arc::new(ProjectRootCache::new(move |path: &Path| {
        resolved.fetch_add(1, Ordering::SeqCst);
        op_host_services::project_root::project_root_for(path)
    }));
    let scans = Arc::new(AtomicUsize::new(0));
    let scanned = Arc::clone(&scans);
    let job: scheduler::ScanJob = Arc::new(move |roots| {
        scanned.fetch_add(1, Ordering::SeqCst);
        roots
            .iter()
            .map(|root| (root.clone(), scan::scan_design_dir(root)))
            .collect()
    });
    let scan = ScanScheduler::with_timing(job, Duration::from_secs(3600), Duration::from_secs(5));
    (
        ProjectsHostState::with_parts(Arc::clone(&cache), scan),
        resolutions,
        scans,
        cache,
    )
}

#[test]
fn no_filesystem_access_on_a_frame_where_nothing_changed() {
    let root = temp_dir("idle");
    let (state, resolutions, scans, _cache) = counting_parts();
    let mut app = DesktopApp::new(None);
    app.projects_host = state;
    for number in 0..10 {
        let path = write_design(&root, &format!("doc-{number}.op"));
        assert!(app.open_document_tab(path));
    }
    assert_eq!(app.documents.len(), 10);
    settle(&mut app);
    assert!(
        resolutions.load(Ordering::SeqCst) >= 10,
        "each tab was resolved once"
    );
    assert_eq!(
        scans.load(Ordering::SeqCst),
        1,
        "one scan for the one project"
    );

    let (resolved_before, scanned_before) = (
        resolutions.load(Ordering::SeqCst),
        scans.load(Ordering::SeqCst),
    );
    for _ in 0..200 {
        assert!(
            !app.publish_projects(),
            "nothing changed, nothing republished"
        );
    }
    assert_eq!(
        resolutions.load(Ordering::SeqCst),
        resolved_before,
        "an idle frame must not resolve a project root"
    );
    assert_eq!(
        scans.load(Ordering::SeqCst),
        scanned_before,
        "an idle frame must not scan the disk"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn root_cache_invalidates_when_a_documents_path_changes() {
    let first_root = temp_dir("move-a");
    let second_root = temp_dir("move-b");
    let old_path = write_design(&first_root, "a.op");
    let new_path = write_design(&second_root, "a.op");
    let (state, resolutions, _scans, cache) = counting_parts();
    let mut app = DesktopApp::new(None);
    app.projects_host = state;
    assert!(app.open_document_tab(old_path.clone()));
    settle(&mut app);
    assert!(cache.contains(&old_path));
    assert_eq!(panel(&app).len(), 1);

    // Save As elsewhere: the document keeps its tab, its path changes.
    app.current_path = Some(new_path.clone());
    settle(&mut app);
    assert!(!cache.contains(&old_path), "the old path's root is dropped");
    assert!(cache.contains(&new_path));
    let projects = panel(&app);
    let moved = projects
        .iter()
        .find(|project| project.root == second_root.to_string_lossy())
        .expect("the new project is listed");
    assert!(moved.files.iter().any(|file| file.open_id.is_some()));
    let before = resolutions.load(Ordering::SeqCst);
    settle(&mut app);
    assert_eq!(resolutions.load(Ordering::SeqCst), before);
    let _ = std::fs::remove_dir_all(first_root);
    let _ = std::fs::remove_dir_all(second_root);
}

fn request_new_design(app: &mut DesktopApp, root: &Path) {
    app.host
        .editor_state_mut()
        .editor_ui
        .projects_panel
        .pending_request = Some(ProjectRequest::NewDesignIn(
        root.to_string_lossy().into_owned(),
    ));
    app.pump_projects();
}

pub(super) fn toast_key(app: &DesktopApp) -> Option<String> {
    app.host
        .editor_state()
        .editor_ui
        .editor_toast
        .as_ref()
        .map(|toast| toast.i18n_key.clone())
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn new_design_failure_shows_a_toast_and_creates_nothing_partial() {
    let root = temp_dir("fail");
    // A plain file where the `designs` folder should be.
    std::fs::write(root.join("designs"), "in the way").unwrap();
    let mut app = DesktopApp::new(None);
    let tabs_before = app.documents.len();
    assert_eq!(toast_key(&app), None);
    request_new_design(&mut app, &root);
    assert_eq!(toast_key(&app).as_deref(), Some("projects.newDesignFailed"));
    assert_eq!(app.documents.len(), tabs_before, "no tab was opened");
    assert_eq!(
        dir_entries(&root),
        ["designs", "package.json"],
        "no file or folder was left behind"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("designs")).unwrap(),
        "in the way"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_successful_new_design_shows_no_failure_toast() {
    let root = temp_dir("ok");
    let mut app = DesktopApp::new(None);
    request_new_design(&mut app, &root);
    assert_eq!(toast_key(&app), None);
    assert!(root.join("designs/untitled.op").is_file());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn double_click_creates_one_design() {
    let root = temp_dir("double");
    let mut app = DesktopApp::new(None);
    request_new_design(&mut app, &root);
    request_new_design(&mut app, &root);
    assert_eq!(
        dir_entries(&root.join("designs")),
        ["untitled.op"],
        "the second click of a double click made no second design"
    );
    // The guard is a short window, not a lock: a later click makes another.
    app.projects_host.new_design_debounce = Duration::ZERO;
    request_new_design(&mut app, &root);
    assert_eq!(
        dir_entries(&root.join("designs")),
        ["untitled-2.op", "untitled.op"]
    );
    let _ = std::fs::remove_dir_all(root);
}
