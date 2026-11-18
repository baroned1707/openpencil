//! Projects dock host side: build the project -> files tree from the open
//! documents and the disk, and execute what the tab asked for.
//!
//! A project is either a folder the user opened (`projects_host_open.rs`;
//! every `*.op` anywhere inside it, found by a recursive walk, plus the Figma
//! `*.fig` files the user can convert to `.op` beside them:
//! `projects_host_fig*.rs`) or the folder
//! `project_root_for` resolves for an open document (its `<root>/designs/*.op`
//! plus any other document open from it). A document inside an opened folder
//! belongs to the deepest such folder, so the tab strip, the dock and the agent
//! agree.
//!
//! A frame does no file-system work here:
//! * the project root of each open document comes from a shared cache
//!   (`op_host_services::project_root::ProjectRootCache`), refreshed in the
//!   background when it ages;
//! * `<root>/designs` and opened folders are listed by a worker thread
//!   (`projects_host_scheduler.rs`) whose finished result the frame only drains;
//!   WHEN it runs (focus regained, after an in-app file operation, a background
//!   tick while focused) is `projects_host_refresh.rs`;
//! * the tree is rebuilt only when `input_key` (open documents, roots, folds,
//!   listing version) changes, and a rescan that finds nothing new does not
//!   change the version.
//!
//! The requests that write the disk do so only when the user asks: `+` (create
//! and open), "New design here" (`projects_host_create_here.rs`), rename
//! (`projects_host_rename.rs`) and move to the Trash (`projects_host_trash.rs`).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, FigStatus, ProjectFileKind, ProjectFileView,
    ProjectRequest, ProjectScanState, ProjectView,
};
use op_host_services::project_root::ProjectRootCache;

use crate::DesktopApp;

#[path = "projects_host_create_here.rs"]
mod create_here;
#[path = "projects_host_fig.rs"]
mod fig;
#[path = "projects_host_fig_app.rs"]
mod fig_app;
#[path = "projects_host_fig_plan.rs"]
mod fig_plan;
#[path = "projects_host_fig_process.rs"]
mod fig_process;
#[path = "projects_host_new_design.rs"]
mod new_design;
#[path = "projects_host_open.rs"]
mod open;
#[path = "projects_host_refresh.rs"]
mod refresh;
#[path = "projects_host_rename.rs"]
mod rename;
#[path = "projects_host_rename_fs.rs"]
mod rename_fs;
#[path = "projects_host_scan.rs"]
mod scan;
#[path = "projects_host_scan_tree.rs"]
mod scan_tree;
#[path = "projects_host_scheduler.rs"]
mod scheduler;
#[path = "projects_host_trash.rs"]
mod trash;
#[cfg(target_os = "macos")]
#[path = "projects_host_trash_macos.rs"]
mod trash_macos;

#[cfg(test)]
pub(crate) use scan::list_design_files;
use scan::ScanStatus;
use scheduler::ScanScheduler;

/// Asks the user for a folder (the argument is the dialog's title); `None` is a
/// cancel. Replaceable so a headless test never reaches a native dialog.
pub(crate) type FolderPicker = Box<dyn FnMut(&str) -> Option<PathBuf>>;

/// A second `+` on the same project inside this window is a double click, not a
/// second design.
const NEW_DESIGN_DEBOUNCE: Duration = Duration::from_secs(1);

/// i18n key of the toast raised when a new design could not be created.
const NEW_DESIGN_FAILED_KEY: &str = "projects.newDesignFailed";

/// Host-side memory behind the tab: which projects exist, which are folded,
/// and the last disk listing.
pub(crate) struct ProjectsHostState {
    /// Every project root seen this session, so closing the last tab of a
    /// project does not make the project vanish from the list.
    known: BTreeSet<PathBuf>,
    collapsed: HashSet<PathBuf>,
    /// Memo of `document path -> project root`; the only way a frame learns a
    /// root.
    roots: Arc<ProjectRootCache>,
    /// The disk listing, kept fresh by a worker thread.
    scan: ScanScheduler,
    /// Hash of everything the published tree was built from; an unchanged
    /// value means there is nothing to rebuild.
    published_key: Option<u64>,
    /// The path each open document had last frame, to drop the old path's root
    /// when a document moves or closes.
    document_paths: HashMap<DocumentId, PathBuf>,
    /// The last accepted `+` per project, for the double-click guard.
    last_new_design: Option<(PathBuf, Instant)>,
    new_design_debounce: Duration,
    /// Statuses already reported, so a persistent condition is logged once.
    logged_status: BTreeMap<PathBuf, ScanStatus>,
    /// Folded folders, by `(project root, relative folder)`.
    collapsed_folders: HashSet<(PathBuf, String)>,
    /// Opened projects whose walk has not finished: the scan serial that will
    /// include them. Only these show "Scanning" (the periodic tick does not).
    awaiting: BTreeMap<PathBuf, u64>,
    /// The folder dialog.
    picker: FolderPicker,
    /// Home directory and system folders the unsafe-root check uses; `None`
    /// means this machine's.
    home_override: Option<Option<PathBuf>>,
    system_dirs: op_host_services::project_root::SystemDirs,
    /// The opened roots last given to the live MCP server.
    mcp_roots_sent: Option<Vec<PathBuf>>,
    /// Opened roots saved by the last run, restored once the window is up.
    pending_restore: Vec<String>,
    /// `.fig` conversions: queued, running and failed (`projects_host_fig.rs`).
    figs: fig::FigJobs,
    /// Opened roots rescanned because a conversion finished in them; once that
    /// scan is done their "done" marks are dropped.
    fig_rescans: BTreeSet<PathBuf>,
    /// Where "Move to Trash" sends a file (`projects_host_trash.rs`).
    trash: Arc<dyn trash::TrashBackend>,
    /// A design just created from the dock: its row gets the inline rename as
    /// soon as the tree that lists it is published.
    pending_rename: Option<PathBuf>,
    /// The last focus event; `None` until there is one (then the window is asked).
    focused: Option<bool>,
}

impl Default for ProjectsHostState {
    fn default() -> Self {
        let roots = op_host_services::project_root::shared_cache();
        let scan = ScanScheduler::new(scheduler::disk_scan_job(Arc::clone(&roots)))
            .with_tree_job(scheduler::tree_scan_job());
        Self::with_parts(roots, scan)
    }
}

impl ProjectsHostState {
    /// A state over the given root cache and scanner (tests inject counting
    /// stand-ins).
    pub(crate) fn with_parts(roots: Arc<ProjectRootCache>, scan: ScanScheduler) -> Self {
        Self {
            known: BTreeSet::new(),
            collapsed: HashSet::new(),
            roots,
            scan,
            published_key: None,
            document_paths: HashMap::new(),
            last_new_design: None,
            new_design_debounce: NEW_DESIGN_DEBOUNCE,
            logged_status: BTreeMap::new(),
            collapsed_folders: HashSet::new(),
            awaiting: BTreeMap::new(),
            picker: open::native_folder_picker(),
            home_override: None,
            system_dirs: op_host_services::project_root::SystemDirs::host(),
            mcp_roots_sent: None,
            pending_restore: Vec::new(),
            figs: fig::FigJobs::new(fig_process::default_converter()),
            fig_rescans: BTreeSet::new(),
            trash: trash::default_backend(),
            pending_rename: None,
            focused: None,
        }
    }
}

/// One open document as the builder sees it.
pub(crate) struct OpenDoc {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub status: DocumentTabStatus,
    pub active: bool,
}

/// `.op` design, or a `.fig` with its conversion status: a running or failed
/// job first, else "converted" when `<stem>.op` is listed beside it.
fn file_kind(
    path: &Path,
    listed: &BTreeSet<PathBuf>,
    jobs: &BTreeMap<PathBuf, fig::JobStatus>,
) -> ProjectFileKind {
    if path.extension().is_none_or(|extension| extension != "fig") {
        return ProjectFileKind::Design;
    }
    let status = match jobs.get(path) {
        Some(fig::JobStatus::Converting) => FigStatus::Converting,
        Some(fig::JobStatus::Done) => FigStatus::Converted,
        _ if listed.contains(&path.with_extension("op")) => FigStatus::Converted,
        Some(fig::JobStatus::Failed(reason)) => FigStatus::Failed(reason.clone()),
        None => FigStatus::NotConverted,
    };
    ProjectFileKind::Figma(status)
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The relative directory of `file` inside `root` with `/` separators; empty
/// for a file directly in the root or outside it.
fn folder_of(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .map(|parent| {
            parent
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default()
}

/// What the tree builder reads. Plain data, so the builder stays free of the
/// file system.
pub(crate) struct TreeInputs<'a> {
    /// Every project to list.
    pub roots: &'a BTreeSet<PathBuf>,
    /// The roots the user opened (a subset of `roots`).
    pub explicit: &'a [PathBuf],
    /// The design files found on disk for each project root.
    pub listing: &'a BTreeMap<PathBuf, Vec<PathBuf>>,
    pub open: &'a [OpenDoc],
    pub collapsed: &'a HashSet<PathBuf>,
    pub collapsed_folders: &'a HashSet<(PathBuf, String)>,
    /// Queued, running and failed `.fig` conversions, by the `.fig` path.
    pub fig_jobs: &'a BTreeMap<PathBuf, fig::JobStatus>,
    /// Open documents whose file is gone from disk.
    pub missing: &'a BTreeSet<PathBuf>,
    /// The platform can move a file to the Trash.
    pub trash_available: bool,
    pub scan_state: &'a dyn Fn(&Path) -> ProjectScanState,
}

/// Pure tree builder: no filesystem access, so it is unit-testable.
///
/// A file found under an opened root that lies inside a DEEPER opened root
/// belongs to that one only. The files of a project are grouped by folder (the
/// root's own files first), so the dock can put a sub-header over each group.
pub(crate) fn build_projects(
    inputs: &TreeInputs<'_>,
    root_of: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<ProjectView> {
    inputs
        .roots
        .iter()
        .map(|root| {
            let deeper = |path: &Path| {
                inputs.explicit.iter().any(|other| {
                    other != root && other.starts_with(root) && path.starts_with(other)
                })
            };
            let mut paths: BTreeSet<PathBuf> = inputs
                .listing
                .get(root)
                .map(|files| files.iter().filter(|path| !deeper(path)).cloned().collect())
                .unwrap_or_default();
            // A document open from this project shows even when it lives
            // outside `designs/`.
            for doc in inputs.open {
                if let Some(path) = doc.path.as_ref() {
                    if root_of(path).as_ref() == Some(root) {
                        paths.insert(path.clone());
                    }
                }
            }
            let paths_set = paths.clone();
            let mut files: Vec<ProjectFileView> = paths
                .into_iter()
                .map(|path| {
                    let doc = inputs.open.iter().find(|d| d.path.as_ref() == Some(&path));
                    let kind = file_kind(&path, &paths_set, inputs.fig_jobs);
                    // Only an open tab is marked: a closed file that vanished is
                    // simply no longer listed.
                    let missing = doc.is_some() && inputs.missing.contains(&path);
                    let removable = inputs.trash_available
                        && !missing
                        && path
                            .extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("op"));
                    ProjectFileView {
                        kind,
                        missing,
                        removable,
                        label: file_label(&path),
                        folder: folder_of(root, &path),
                        path: path.to_string_lossy().into_owned(),
                        open_id: doc.map(|d| d.id),
                        dirty: doc.is_some_and(|d| d.dirty),
                        status: doc.map_or(DocumentTabStatus::Idle, |d| d.status),
                        active: doc.is_some_and(|d| d.active),
                    }
                })
                .collect();
            // Stable: within a folder the sorted-by-path order stays.
            files.sort_by(|a, b| a.folder.cmp(&b.folder));
            ProjectView {
                root: root.to_string_lossy().into_owned(),
                name: file_label(root),
                files,
                expanded: !inputs.collapsed.contains(root),
                closable: inputs.explicit.contains(root),
                scan: (inputs.scan_state)(root),
                collapsed_folders: inputs
                    .collapsed_folders
                    .iter()
                    .filter(|(project, _)| project == root)
                    .map(|(_, folder)| folder.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            }
        })
        .collect()
}

impl ProjectsHostState {
    /// Whether a `.fig` conversion is queued or running (the event loop keeps
    /// waking to poll it) or a finished one still waits for its rescan.
    pub(crate) fn figs_pending(&self) -> bool {
        self.figs.has_work() || !self.fig_rescans.is_empty()
    }

    /// Replace the `.fig` converter and its limits (tests inject fakes, so no
    /// test ever runs the test binary as a converter).
    #[cfg(test)]
    pub(crate) fn set_fig_converter(
        &mut self,
        converter: Arc<dyn fig::FigConverter>,
        limits: fig::FigLimits,
    ) {
        self.figs.set_converter(converter);
        self.figs.set_limits(limits);
    }

    /// Replace the Trash backend (tests inject a recording fake, so no test
    /// ever touches the real Trash).
    #[cfg(test)]
    pub(crate) fn set_trash_backend(&mut self, backend: Arc<dyn trash::TrashBackend>) {
        self.trash = backend;
    }

    /// Forget the cached root of every path a document no longer has (it was
    /// saved elsewhere, renamed or closed). Memory only.
    fn track_document_paths(&mut self, open: &[OpenDoc]) {
        let unchanged = open.iter().filter(|doc| doc.path.is_some()).count()
            == self.document_paths.len()
            && open.iter().all(|doc| match doc.path.as_ref() {
                Some(path) => self.document_paths.get(&doc.id) == Some(path),
                None => true,
            });
        if unchanged {
            return;
        }
        let current: HashMap<DocumentId, PathBuf> = open
            .iter()
            .filter_map(|doc| doc.path.clone().map(|path| (doc.id, path)))
            .collect();
        for (id, old_path) in &self.document_paths {
            if current.get(id) != Some(old_path) {
                self.roots.invalidate(old_path);
            }
        }
        // A document bound to a path the dock does not list yet (a Save As, or a
        // file created by an agent) is a file operation of this app: look at
        // the disk again. A tab opened on a listed file asks for nothing.
        let unlisted_new_path = current.iter().any(|(id, path)| {
            self.document_paths.get(id) != Some(path) && !self.scan.is_listed(path)
        });
        self.document_paths = current;
        if unlisted_new_path {
            self.scan.request_rescan_all();
        }
    }

    /// How far a project's scan got, as the dock shows it.
    fn scan_state(&self, root: &Path) -> ProjectScanState {
        let status = self.scan.status(root).unwrap_or(ScanStatus::Ok);
        let waiting = self
            .awaiting
            .get(root)
            .is_some_and(|serial| *serial > self.scan.completed_serial());
        match status {
            ScanStatus::Slow => ProjectScanState::Slow,
            _ if waiting => ProjectScanState::Scanning,
            ScanStatus::Ok => ProjectScanState::Idle,
            ScanStatus::Unreadable(_) => ProjectScanState::Unreadable,
            ScanStatus::Truncated(omitted) => ProjectScanState::Truncated(omitted),
        }
    }

    /// Everything the published tree depends on, as one number.
    fn input_key(
        &self,
        open: &[OpenDoc],
        doc_roots: &[Option<PathBuf>],
        explicit_generation: u64,
    ) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (doc, root) in open.iter().zip(doc_roots) {
            doc.id.hash(&mut hasher);
            doc.path.hash(&mut hasher);
            doc.dirty.hash(&mut hasher);
            (doc.status as u8).hash(&mut hasher);
            doc.active.hash(&mut hasher);
            root.hash(&mut hasher);
        }
        self.known.hash(&mut hasher);
        // `HashSet` iterates in no fixed order; sort for a stable hash.
        let mut collapsed: Vec<&PathBuf> = self.collapsed.iter().collect();
        collapsed.sort();
        collapsed.hash(&mut hasher);
        let mut folders: Vec<&(PathBuf, String)> = self.collapsed_folders.iter().collect();
        folders.sort();
        folders.hash(&mut hasher);
        explicit_generation.hash(&mut hasher);
        self.scan.version().hash(&mut hasher);
        self.figs.version().hash(&mut hasher);
        // Whether each opened walk is still pending changes what the dock says.
        let completed = self.scan.completed_serial();
        for (root, serial) in &self.awaiting {
            (root, *serial > completed).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Log a project's scan status when it changes to something other than Ok.
    fn log_scan_status_changes(&mut self) {
        for root in &self.known {
            let status = self.scan.status(root).unwrap_or(ScanStatus::Ok);
            let previous = self
                .logged_status
                .get(root)
                .copied()
                .unwrap_or(ScanStatus::Ok);
            if status != previous {
                if status != ScanStatus::Ok {
                    eprintln!("[projects] {}: {status}", root.display());
                }
                self.logged_status.insert(root.clone(), status);
            }
        }
    }
}

impl DesktopApp {
    fn open_docs(&mut self) -> Vec<OpenDoc> {
        self.sync_active_slot();
        let live_dirty = self.document_is_dirty();
        let tabs: Vec<DocumentTabView> = self.documents.tab_views(live_dirty, "");
        let paths = self.documents.slot_paths();
        tabs.into_iter()
            .zip(paths)
            .map(|(tab, path)| OpenDoc {
                id: tab.id,
                path,
                dirty: tab.dirty,
                status: tab.status,
                active: tab.active,
            })
            .collect()
    }

    /// Execute the Projects dock's request, then publish the tree. Returns
    /// whether anything changed. Runs once per frame next to the tab strip.
    pub(crate) fn pump_projects(&mut self) -> bool {
        let mut changed = self.pump_fig_conversions();
        // An inline rename (or a "Move to Trash?" question) whose row is gone
        // (or under Home) must not keep the keyboard.
        let ui = &mut self.host.editor_state_mut().editor_ui;
        if ui.home.visible {
            changed |= ui.projects_panel.cancel_rename();
            changed |= ui.projects_panel.cancel_trash_confirm();
            // The quick switcher is modal: it must not outlive the surface
            // that covers it (a menu action can show Home with no key press).
            changed |= ui.projects_panel.switcher.close();
        }
        changed |= ui.projects_panel.drop_hidden_rename();
        changed |= ui.projects_panel.drop_hidden_trash_confirm();
        // A rename the dock confirmed runs before the same frame's request: a
        // click-away commit and the press that caused it arrive together.
        changed |= self.pump_project_rename();
        changed |= self.pump_project_trash();
        if let Some(request) = self
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .pending_request
            .take()
        {
            self.execute_project_request(request);
            changed = true;
        }
        changed |= self.publish_projects();
        changed | self.start_pending_rename()
    }

    /// A design was just created from the dock: once its row is published,
    /// start renaming it. The row is in the listing already (patched in), so a
    /// miss means the project vanished; the intent is dropped then.
    fn start_pending_rename(&mut self) -> bool {
        let Some(path) = self.projects_host.pending_rename.take() else {
            return false;
        };
        self.host.begin_project_rename(&path.to_string_lossy())
    }

    fn execute_project_request(&mut self, request: ProjectRequest) {
        match request {
            ProjectRequest::SwitchTo(id) => {
                self.sync_active_slot();
                if let Some(index) = self.documents.index_of(id) {
                    self.activate_document_slot(index);
                }
            }
            ProjectRequest::OpenFile(path) => {
                self.open_document_tab(PathBuf::from(path));
            }
            ProjectRequest::ToggleProject(root) => {
                let root = PathBuf::from(root);
                if !self.projects_host.collapsed.remove(&root) {
                    self.projects_host.collapsed.insert(root);
                }
            }
            ProjectRequest::NewDesignIn(root) => self.create_design_in(Path::new(&root)),
            ProjectRequest::NewDesignHere(root, folder) => {
                self.create_design_here(Path::new(&root), &folder);
            }
            ProjectRequest::OpenProjectPicker => {
                self.open_project_via_picker();
            }
            ProjectRequest::CloseProject(root) => self.close_project(Path::new(&root)),
            ProjectRequest::RefreshProject(root) => self.refresh_project(Path::new(&root)),
            ProjectRequest::ConvertFig(path) => self.convert_fig_clicked(Path::new(&path)),
            ProjectRequest::ConvertAllFigs(root, folder) => {
                self.convert_all_figs(Path::new(&root), folder.as_deref());
            }
            ProjectRequest::ToggleFolder(root, folder) => {
                let key = (PathBuf::from(root), folder);
                if !self.projects_host.collapsed_folders.remove(&key) {
                    self.projects_host.collapsed_folders.insert(key);
                }
            }
        }
        // A request can change what is open or folded.
        self.projects_host.published_key = None;
        self.projects_host.scan.request_now();
    }

    /// Create `<root>/designs/untitled[-n].op` with a starter document and open
    /// it in a new tab. Refuses (toast) while a tab switch is blocked; a failed
    /// creation shows a toast and leaves nothing behind.
    fn create_design_in(&mut self, root: &Path) {
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return;
        }
        let state = &mut self.projects_host;
        let now = Instant::now();
        if let Some((last_root, at)) = state.last_new_design.as_ref() {
            if last_root == root && now.saturating_duration_since(*at) < state.new_design_debounce {
                return;
            }
        }
        state.last_new_design = Some((root.to_path_buf(), now));
        let starter = op_editor_core::EditorState::starter();
        let created = new_design::create_design_file(root, &|target| {
            op_host_services::doc_io::save_new_to_path(&starter, target)
        });
        match created {
            Ok(target) => {
                self.projects_host.known.insert(root.to_path_buf());
                // The target's folder may not have existed when its root was
                // last resolved.
                self.projects_host.roots.invalidate(&target);
                // An opened project is walked on request only, so ask: the
                // file must stay listed after its tab closes.
                if self.documents.explicit_roots().contains(root) {
                    self.refresh_project(root);
                }
                self.rescan_after_own_operation();
                self.open_document_tab(target);
            }
            Err(error) => {
                eprintln!("[projects] new design in {}: {error}", root.display());
                let now_ms = self.host.now_ms();
                self.host.editor_state_mut().editor_ui.show_toast(
                    NEW_DESIGN_FAILED_KEY,
                    vec![("reason".to_string(), error.to_string())],
                    EditorToastLevel::Warn,
                    now_ms,
                );
                self.host.mark_editor_state_dirty();
                self.request_redraw(true);
            }
        }
    }

    /// The project roots the dock lists. An inferred project inside an opened
    /// folder is part of that folder, so it is not listed on its own.
    pub(crate) fn shown_project_roots(&self) -> BTreeSet<PathBuf> {
        let explicit_roots = self.documents.explicit_roots().snapshot();
        self.projects_host
            .known
            .iter()
            .filter(|root| {
                explicit_roots.contains(root)
                    || !explicit_roots.iter().any(|other| root.starts_with(other))
            })
            .cloned()
            .collect()
    }

    /// Mirror open documents and the disk into `editor_ui.projects_panel`.
    pub(crate) fn publish_projects(&mut self) -> bool {
        let open = self.open_docs();
        self.projects_host.track_document_paths(&open);
        let explicit = Arc::clone(self.documents.explicit_roots());
        let doc_roots: Vec<Option<PathBuf>> = open
            .iter()
            .map(|doc| {
                let path = doc.path.as_deref()?;
                explicit.root_for(path).or_else(|| {
                    self.projects_host
                        .roots
                        .get(path)
                        .map(|project| project.root)
                })
            })
            .collect();
        for root in doc_roots.iter().flatten() {
            if self.projects_host.known.insert(root.clone()) {
                self.projects_host.scan.request_now();
            }
        }
        let explicit_roots = explicit.snapshot();
        let shown = self.shown_project_roots();
        let active = self.projects_background_active();
        let watched: Vec<PathBuf> = open.iter().filter_map(|doc| doc.path.clone()).collect();
        let state = &mut self.projects_host;
        state.scan.set_watched(watched);
        state.scan.poll(&shown, || active);
        let completed = state.scan.completed_serial();
        state.awaiting.retain(|_, serial| *serial > completed);
        let scanning = state.scan.in_flight();
        state.log_scan_status_changes();
        if scanning {
            // The worker cannot wake the event loop, so keep frames coming
            // until its result is drained.
            self.request_redraw(false);
        }
        let key = self
            .projects_host
            .input_key(&open, &doc_roots, explicit.generation());
        let panel_rows = self
            .host
            .editor_state()
            .editor_ui
            .projects_panel
            .projects
            .len();
        if self.projects_host.published_key == Some(key) && panel_rows == shown.len() {
            return false;
        }
        let root_by_path: HashMap<&Path, &PathBuf> = open
            .iter()
            .zip(&doc_roots)
            .filter_map(|(doc, root)| Some((doc.path.as_deref()?, root.as_ref()?)))
            .collect();
        let host = &self.projects_host;
        let scan_state = |root: &Path| host.scan_state(root);
        let trash_available = host.trash.supported();
        let projects = build_projects(
            &TreeInputs {
                roots: &shown,
                explicit: &explicit_roots,
                listing: host.scan.files(),
                open: &open,
                collapsed: &host.collapsed,
                collapsed_folders: &host.collapsed_folders,
                fig_jobs: host.figs.status(),
                missing: host.scan.missing(),
                trash_available,
                scan_state: &scan_state,
            },
            &|path| root_by_path.get(path).map(|root| (*root).clone()),
        );
        self.projects_host.published_key = Some(key);
        let panel = &mut self.host.editor_state_mut().editor_ui.projects_panel;
        if panel.projects == projects {
            return false;
        }
        panel.projects = projects;
        self.host.mark_editor_state_dirty();
        true
    }
}

#[cfg(test)]
#[path = "projects_host_fig_app_tests.rs"]
mod fig_app_tests;
#[cfg(test)]
#[path = "projects_host_open_tests.rs"]
mod open_tests;
#[cfg(test)]
#[path = "projects_host_tests.rs"]
mod tests;
