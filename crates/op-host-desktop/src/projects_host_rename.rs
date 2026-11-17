//! Renaming a design from the Projects dock, on the desktop side.
//!
//! The dock only records a confirmed name (`ProjectsPanelState::rename_commit`,
//! its own slot beside `pending_request`). This file executes it, and the order
//! is the point: every check runs against the live state and the disk first
//! (`projects_host_rename_fs.rs`), the file is renamed, and ONLY THEN does
//! anything that remembers the old path change. A failure at any step before
//! the disk rename leaves the row, the tab and the disk exactly as they were.
//!
//! What follows a renamed file, so nothing keeps pointing at a name that is gone:
//! * the tab bound to it (on screen or parked): its path, the file name the
//!   chrome shows, the window title and its Git binding. The document itself
//!   is not touched: unsaved edits stay unsaved, undo history and agent runs
//!   stay as they are, and nothing is re-read from disk;
//! * its recent-files entry;
//! * the crash-recovery manifest of an unsaved tab (it stores the path);
//! * the dock's cached listing and the project-root memo.
//!
//! A file open in a tab is renamed only while no save is being written (a
//! queued or running save replaces the OLD path atomically and would bring it
//! back) and while exactly one tab is bound to it.

use std::path::{Path, PathBuf};

use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{ProjectRenameRequest, RenameNameError};

use super::rename_fs::{execute_rename, plan_rename, RenamePlan, RenameRefusal};
use crate::DesktopApp;

/// i18n key of the toast raised when a rename failed after the name was accepted.
const RENAME_FAILED_KEY: &str = "projects.renameFailed";

impl DesktopApp {
    /// Execute the rename the dock confirmed, if any. Returns whether the
    /// request was taken (the tree needs a republish either way).
    pub(crate) fn pump_project_rename(&mut self) -> bool {
        let Some(request) = self
            .host
            .editor_state_mut()
            .editor_ui
            .projects_panel
            .rename_commit
            .take()
        else {
            return false;
        };
        self.execute_project_rename(request);
        self.projects_host.published_key = None;
        self.projects_host.scan.request_now();
        true
    }

    fn execute_project_rename(&mut self, request: ProjectRenameRequest) {
        let ProjectRenameRequest { path, new_stem } = request;
        let outcome = self.rename_design_file(Path::new(&path), &new_stem);
        let panel = &mut self.host.editor_state_mut().editor_ui.projects_panel;
        match outcome {
            Ok(()) => panel.finish_rename(&path),
            // The name is the problem: the field stays open and says so.
            Err(RenameRefusal::NameTaken) => panel.reject_rename(&path, RenameNameError::Taken),
            Err(RenameRefusal::InvalidName(error)) => panel.reject_rename(&path, error),
            // Anything else is not about the name: tell the user and close the
            // field, leaving the row as it was.
            Err(refusal) => {
                panel.finish_rename(&path);
                eprintln!("[projects] rename {path}: {refusal}");
                let now_ms = self.host.now_ms();
                self.host.editor_state_mut().editor_ui.show_toast(
                    RENAME_FAILED_KEY,
                    vec![("reason".to_string(), refusal.to_string())],
                    EditorToastLevel::Warn,
                    now_ms,
                );
            }
        }
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }

    /// Rename the design at `source` to `<new_stem>.op` in the same folder.
    /// Re-validates everything; the widget's checks are not trusted.
    pub(crate) fn rename_design_file(
        &mut self,
        source: &Path,
        new_stem: &str,
    ) -> Result<(), RenameRefusal> {
        let roots: Vec<PathBuf> = self.shown_project_roots().into_iter().collect();
        let Some(plan) = plan_rename(&roots, source, new_stem)? else {
            return Ok(());
        };
        if self.projects_host.figs.is_writing(&plan.source) {
            return Err(RenameRefusal::ConversionRunning);
        }
        self.sync_active_slot();
        let tabs = self.documents.indices_of_path(&plan.source);
        if tabs.len() > 1 {
            return Err(RenameRefusal::OpenInSeveralTabs);
        }
        if !tabs.is_empty() && self.save_session.is_active() {
            return Err(RenameRefusal::SaveInProgress);
        }
        execute_rename(&plan)?;
        self.follow_renamed_file(tabs.first().copied(), &plan);
        Ok(())
    }

    /// The disk rename succeeded: move everything that remembered the old path.
    fn follow_renamed_file(&mut self, tab: Option<usize>, plan: &RenamePlan) {
        let RenamePlan { source, target, .. } = plan;
        if let Some(index) = tab {
            self.follow_renamed_tab(index, target);
        }
        crate::settings_io::rename_recent(&mut self.host, source, target);
        self.projects_host.roots.invalidate(source);
        self.projects_host.roots.invalidate(target);
        self.projects_host.scan.rename_listed(source, target);
        self.publish_document_tabs();
    }

    /// Bind the tab at `index` to `target`. Only the bound path and what is
    /// derived from it change.
    fn follow_renamed_tab(&mut self, index: usize, target: &Path) {
        let id = self.documents.id_at(index);
        if index == self.documents.active_index() {
            self.current_path = Some(target.to_path_buf());
            op_host_services::doc_io::set_file_name_display(
                self.host.editor_state_mut(),
                Some(target),
            );
            crate::persistence::refresh_title(&self.current_path, self.window.as_ref());
            self.rebind_git_session_for_current_path();
        } else if let Some(slot) = self.documents.slot_mut(index) {
            slot.local.path = Some(target.to_path_buf());
            if let Some(git) = slot.local.git_session.as_mut() {
                git.rebind(Some(target));
            }
            if let Some(parked) = slot.parked.as_mut() {
                parked.scope_mut().set_file_name_display(
                    target
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                );
            }
        }
        if let Some(id) = id {
            self.recovery_follow_renamed_path(id);
        }
        self.host.mark_editor_state_dirty();
    }
}

#[cfg(test)]
#[path = "projects_host_rename_tests.rs"]
mod tests;
