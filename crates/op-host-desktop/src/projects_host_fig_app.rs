//! The Projects dock's `.fig` requests, executed on the app: a click on a
//! `.fig` row, "Convert all", and the per-frame pump of the conversion queue.
//!
//! Conversion is only ever user-triggered. A single click converts that file and
//! opens the result; a batch converts every waiting file in its scope and never
//! opens a tab. Nothing here converts anything itself: it validates the request
//! against the scan and the disk, hands the file to the queue
//! (`projects_host_fig.rs`) and reacts to what finishes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use op_editor_core::in_folder_tree;

use super::fig_plan::{plan_conversion, Plan};
use crate::DesktopApp;

impl DesktopApp {
    /// The opened root whose scan listed `fig`, or `None` for a path the dock
    /// never showed (a stale or forged request): only listed files convert.
    fn listed_fig_root(&self, fig: &Path) -> Option<PathBuf> {
        let root = self.documents.explicit_roots().root_for(fig)?;
        let listed = self
            .projects_host
            .scan
            .files()
            .get(&root)
            .is_some_and(|files| files.iter().any(|path| path == fig));
        listed.then_some(root)
    }

    /// A click on a `.fig` row. With its `.op` already beside it that file
    /// opens; otherwise the conversion is queued (a failed one retries) and the
    /// result opens when it lands.
    pub(crate) fn convert_fig_clicked(&mut self, fig: &Path) {
        let Some(root) = self.listed_fig_root(fig) else {
            eprintln!(
                "[projects] ignoring a convert request for {}",
                fig.display()
            );
            return;
        };
        if let Ok(Plan::AlreadyConverted(output)) = plan_conversion(&root, fig) {
            self.open_document_tab(output);
            return;
        }
        self.queue_fig(fig.to_path_buf(), root, true);
    }

    /// "Convert all": queue every not-yet-converted `.fig` the dock shows under
    /// `folder` (and below) in the project with this root, or in the whole
    /// project. Never opens a tab.
    pub(crate) fn convert_all_figs(&mut self, root: &Path, folder: Option<&str>) {
        if !self.documents.explicit_roots().contains(root) {
            return;
        }
        let root_text = root.to_string_lossy();
        let waiting: Vec<PathBuf> = self
            .host
            .editor_state()
            .editor_ui
            .projects_panel
            .projects
            .iter()
            .filter(|project| project.root == root_text)
            .flat_map(|project| project.files.iter())
            .filter(|file| file.kind.is_not_converted() && in_folder_tree(&file.folder, folder))
            .map(|file| PathBuf::from(&file.path))
            .collect();
        let listed: HashSet<&PathBuf> = self
            .projects_host
            .scan
            .files()
            .get(root)
            .map(|files| files.iter().collect())
            .unwrap_or_default();
        let queued: Vec<PathBuf> = waiting
            .into_iter()
            .filter(|path| listed.contains(path))
            .collect();
        for fig in queued {
            self.queue_fig(fig, root.to_path_buf(), false);
        }
    }

    fn queue_fig(&mut self, fig: PathBuf, root: PathBuf, open_when_done: bool) {
        if self.projects_host.figs.enqueue(fig, root, open_when_done) {
            self.projects_host.published_key = None;
            self.request_redraw(true);
        }
    }

    /// One frame of conversion bookkeeping: poll the children, start queued
    /// ones, rescan a project whose conversion finished (so its `.op` is listed)
    /// and open the result of a single-file click. Returns whether the dock may
    /// have changed.
    pub(crate) fn pump_fig_conversions(&mut self) -> bool {
        let state = &self.projects_host;
        if !state.figs.has_work() && state.fig_rescans.is_empty() {
            return false;
        }
        let version = self.projects_host.figs.version();
        let finished = self.projects_host.figs.pump(Instant::now());
        for done in finished {
            self.projects_host.fig_rescans.insert(done.root.clone());
            // The folder may not have held a design when its root was resolved.
            self.projects_host.roots.invalidate(&done.output);
            self.refresh_project(&done.root);
            if done.open_when_done {
                self.open_document_tab(done.output);
            }
        }
        let scanned: Vec<PathBuf> = self
            .projects_host
            .fig_rescans
            .iter()
            .filter(|root| !self.projects_host.awaiting.contains_key(*root))
            .cloned()
            .collect();
        for root in scanned {
            self.projects_host.figs.clear_done(&root);
            self.projects_host.fig_rescans.remove(&root);
        }
        let changed = self.projects_host.figs.version() != version;
        if changed {
            self.request_redraw(true);
        }
        changed
    }
}
