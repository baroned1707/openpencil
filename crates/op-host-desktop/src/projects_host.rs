//! Projects tab host side: build the project -> files tree from the open
//! documents and the disk, and execute what the tab asked for.
//!
//! A project is the folder `project_root_for` resolves for a file. Its files
//! are every `*.op` in `<root>/designs/` plus any other document open from it.
//! The tree is rebuilt only when its inputs change (open documents, known
//! projects, collapse flags) or the periodic disk rescan comes due, so a frame
//! costs two comparisons.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, ProjectFileView, ProjectRequest, ProjectView,
};

use crate::DesktopApp;

/// How often `<root>/designs` is re-read while nothing else changed, so a file
/// an agent created from outside the editor shows up.
const RESCAN_EVERY: Duration = Duration::from_secs(3);

/// Host-side memory behind the tab: which projects exist, which are folded,
/// and the last disk listing.
#[derive(Default)]
pub(crate) struct ProjectsHostState {
    /// Every project root seen this session, so closing the last tab of a
    /// project does not make the project vanish from the list.
    known: BTreeSet<PathBuf>,
    collapsed: HashSet<PathBuf>,
    listing: BTreeMap<PathBuf, Vec<PathBuf>>,
    scanned_at: Option<Instant>,
    published_key: Option<u64>,
}

/// One open document as the builder sees it.
pub(crate) struct OpenDoc {
    pub id: DocumentId,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub status: DocumentTabStatus,
    pub active: bool,
}

/// `<root>/designs/*.op`, sorted by name. Missing or unreadable folders give
/// an empty list.
pub(crate) fn list_design_files(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root.join("designs")) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "op"))
        .collect();
    files.sort();
    files
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Pure tree builder: no filesystem access, so it is unit-testable.
///
/// `listing` maps each project root to the design files found on disk.
pub(crate) fn build_projects(
    roots: &BTreeSet<PathBuf>,
    listing: &BTreeMap<PathBuf, Vec<PathBuf>>,
    open: &[OpenDoc],
    collapsed: &HashSet<PathBuf>,
    root_of: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<ProjectView> {
    roots
        .iter()
        .map(|root| {
            let mut paths: BTreeSet<PathBuf> = listing
                .get(root)
                .map(|files| files.iter().cloned().collect())
                .unwrap_or_default();
            // A document open from this project shows even when it lives
            // outside `designs/`.
            for doc in open {
                if let Some(path) = doc.path.as_ref() {
                    if root_of(path).as_ref() == Some(root) {
                        paths.insert(path.clone());
                    }
                }
            }
            let files = paths
                .into_iter()
                .map(|path| {
                    let doc = open.iter().find(|d| d.path.as_ref() == Some(&path));
                    ProjectFileView {
                        label: file_label(&path),
                        path: path.to_string_lossy().into_owned(),
                        open_id: doc.map(|d| d.id),
                        dirty: doc.is_some_and(|d| d.dirty),
                        status: doc.map_or(DocumentTabStatus::Idle, |d| d.status),
                        active: doc.is_some_and(|d| d.active),
                    }
                })
                .collect();
            ProjectView {
                root: root.to_string_lossy().into_owned(),
                name: file_label(root),
                files,
                expanded: !collapsed.contains(root),
            }
        })
        .collect()
}

/// A path for a new design in `root` that does not exist yet:
/// `untitled.op`, then `untitled-2.op`, ...
pub(crate) fn next_untitled_design(root: &Path) -> PathBuf {
    let designs = root.join("designs");
    let first = designs.join("untitled.op");
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| designs.join(format!("untitled-{n}.op")))
        .find(|path| !path.exists())
        .expect("an unused name exists")
}

fn root_of_path(path: &Path) -> Option<PathBuf> {
    op_host_services::project_root::project_root_for(path).map(|project| project.root)
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

    /// Execute the Projects tab's request, then publish the tree. Returns
    /// whether anything changed. Runs once per frame next to the tab strip.
    pub(crate) fn pump_projects(&mut self) -> bool {
        let mut changed = false;
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
        changed | self.publish_projects()
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
        }
        // A request can change what is open or folded.
        self.projects_host.published_key = None;
    }

    /// Create `<root>/designs/untitled[-n].op` with a starter document and open
    /// it in a new tab. Refuses (toast) while a tab switch is blocked.
    fn create_design_in(&mut self, root: &Path) {
        if let Some(reason) = self.document_switch_blocked() {
            self.refuse_document_switch(reason);
            return;
        }
        let target = next_untitled_design(root);
        if let Some(dir) = target.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let starter = op_editor_core::EditorState::starter();
        if op_host_services::doc_io::save_new_to_path(&starter, &target).is_ok() {
            self.projects_host.known.insert(root.to_path_buf());
            self.projects_host.scanned_at = None;
            self.open_document_tab(target);
        }
    }

    /// Mirror open documents and the disk into `editor_ui.projects_panel`.
    pub(crate) fn publish_projects(&mut self) -> bool {
        let open = self.open_docs();
        for doc in &open {
            if let Some(root) = doc.path.as_deref().and_then(root_of_path) {
                self.projects_host.known.insert(root);
            }
        }
        let due = self
            .projects_host
            .scanned_at
            .map_or(true, |at| at.elapsed() >= RESCAN_EVERY);
        if due {
            let listing: BTreeMap<PathBuf, Vec<PathBuf>> = self
                .projects_host
                .known
                .iter()
                .map(|root| (root.clone(), list_design_files(root)))
                .collect();
            self.projects_host.listing = listing;
            self.projects_host.scanned_at = Some(Instant::now());
        }
        let projects = build_projects(
            &self.projects_host.known,
            &self.projects_host.listing,
            &open,
            &self.projects_host.collapsed,
            &root_of_path,
        );
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
#[path = "projects_host_tests.rs"]
mod tests;
