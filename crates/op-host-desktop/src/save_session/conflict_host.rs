//! The user-save entry points (Cmd+S / File > Save, the Save button of the
//! close / quit prompts, Save As), each guarded against replacing a file that
//! changed on disk behind the editor's back. See `conflict.rs` for the
//! decision; this file applies it.
//!
//! | file on disk vs the editor's record   | result                                |
//! |----------------------------------------|---------------------------------------|
//! | record matches                         | save, no prompt                       |
//! | file missing                           | save (recreate), no prompt            |
//! | changed, or no usable record           | ONE prompt: Overwrite / Save a copy / Cancel |
//! | an own save to that path is in flight  | no prompt (already approved)          |

use std::path::{Path, PathBuf};

use super::conflict::{classify, default_ui, unique_copy_path, ConflictChoice, ConflictUi};
use super::{can_skip_unchanged_current_save, collaboration_fork_target_is_safe, EnqueueOutcome};
use crate::DesktopApp;

/// What the guard decided for one user save.
enum SaveGuard {
    Proceed,
    Cancel,
    /// Write what is on screen to this new, unused path instead.
    SaveCopy(PathBuf),
}

impl DesktopApp {
    /// Queue Cmd+S without running JSON serialization on the winit thread.
    pub(crate) fn request_background_save(&mut self) -> bool {
        self.request_background_save_with(&mut default_ui())
    }

    /// [`Self::request_background_save`] with the conflict question asked
    /// through `ui`, so the flow runs under test without a native dialog.
    pub(crate) fn request_background_save_with(&mut self, ui: &mut dyn ConflictUi) -> bool {
        let gate = if self.current_path.is_some() {
            op_editor_core::CollabGateAction::SaveShared
        } else {
            op_editor_core::CollabGateAction::SaveFork
        };
        if !self
            .host
            .gate_collaboration_action(gate, op_editor_core::CollabEditSource::User)
        {
            return false;
        }
        let document_epoch = self.host.document_epoch();
        let path = self
            .save_session
            .latest_target(document_epoch)
            .map(Path::to_path_buf)
            .or_else(|| self.current_path.clone());
        let Some(path) = path else {
            return self.request_background_save_as();
        };
        if can_skip_unchanged_current_save(
            self.host.editor_state(),
            self.current_path.as_deref(),
            &path,
            self.save_session.is_active(),
        ) {
            eprintln!("[save] skipped unchanged document");
            return true;
        }
        match self.guard_user_save(&path, ui) {
            SaveGuard::Proceed => {}
            SaveGuard::Cancel => return false,
            SaveGuard::SaveCopy(copy) => {
                // A copy binds the document to a new file: that is the fork
                // authority Save As needs, which plain Save does not.
                if !self.host.gate_collaboration_action(
                    op_editor_core::CollabGateAction::SaveFork,
                    op_editor_core::CollabEditSource::User,
                ) {
                    return false;
                }
                let fork =
                    crate::persistence::requires_background_save_as_fork(self.host.editor_state());
                return self.save_as_target(copy, fork, ui, true);
            }
        }
        let set_current_path = self.current_path.as_deref() != Some(path.as_path());
        let _ = self.enqueue_background_save(path, set_current_path);
        true
    }

    /// Show the native picker synchronously, then queue only the expensive
    /// snapshot serialization and disk write.
    pub(crate) fn request_background_save_as(&mut self) -> bool {
        if !self.host.gate_collaboration_action(
            op_editor_core::CollabGateAction::SaveFork,
            op_editor_core::CollabEditSource::User,
        ) {
            return false;
        }
        let collaboration_bound =
            crate::persistence::requires_background_save_as_fork(self.host.editor_state());
        let Some(path) = crate::persistence::pick_save_as_path(self.host.editor_state()) else {
            return false;
        };
        self.save_as_to_picked_path(path, collaboration_bound)
    }

    /// Everything Save As does once the user has chosen `path`: refuse a file
    /// another tab holds, refuse an unsafe collaboration fork target, ask when
    /// the target is an existing file the editor cannot vouch for, then queue
    /// the write. Split from the picker so it can run without a dialog.
    pub(crate) fn save_as_to_picked_path(
        &mut self,
        path: PathBuf,
        collaboration_bound: bool,
    ) -> bool {
        self.save_as_to_picked_path_with(path, collaboration_bound, &mut default_ui())
    }

    /// [`Self::save_as_to_picked_path`] with the conflict question asked
    /// through `ui`.
    pub(crate) fn save_as_to_picked_path_with(
        &mut self,
        path: PathBuf,
        collaboration_bound: bool,
        ui: &mut dyn ConflictUi,
    ) -> bool {
        self.save_as_target(path, collaboration_bound, ui, false)
    }

    /// `canonical_only` writes what is on screen even for a clean document (the
    /// clean-copy shortcut copies the source file's CURRENT disk bytes, which
    /// after an external change are not what the user is looking at).
    fn save_as_target(
        &mut self,
        path: PathBuf,
        collaboration_bound: bool,
        ui: &mut dyn ConflictUi,
        canonical_only: bool,
    ) -> bool {
        if self.refuse_save_as_onto_file_open_in_another_tab(&path) {
            return false;
        }
        if collaboration_bound
            && !collaboration_fork_target_is_safe(self.current_path.as_deref(), &path)
        {
            let detail =
                "Save As must use a different, resolvable path while collaboration is active.";
            eprintln!(
                "[save] rejected collaboration fork target {}: {detail}",
                path.display()
            );
            crate::persistence::show_error_dialog_public(
                &self.host,
                op_host_services::doc_io::ErrorKind::Save,
                Some(&path),
                detail,
            );
            return false;
        }
        let (path, canonical_only) = match self.guard_user_save(&path, ui) {
            SaveGuard::Proceed => (path, canonical_only),
            SaveGuard::Cancel => return false,
            SaveGuard::SaveCopy(copy) => {
                return self.save_as_target(copy, collaboration_bound, ui, true);
            }
        };
        let identity = (
            self.host.document_epoch(),
            self.host.editor_state().document_generation(),
            self.host.editor_state().document_revision(),
            path.clone(),
        );
        let outcome = self.enqueue_background_save_with(path, true, canonical_only);
        if collaboration_bound {
            return self.confirm_collaboration_fork_enqueue(identity, outcome);
        }
        true
    }

    /// Decide whether a user save to `path` may go ahead, asking once when the
    /// file changed on disk or the editor has no usable record of it.
    fn guard_user_save(&mut self, path: &Path, ui: &mut dyn ConflictUi) -> SaveGuard {
        // A save of this document to this very path is already running or
        // queued: its own write is what the record is about to be refreshed
        // with, and the user already approved it.
        if self.save_session.targets_path(path) {
            return SaveGuard::Proceed;
        }
        let Some(conflict) = classify(self.disk_baselines.get(path), path) else {
            return SaveGuard::Proceed;
        };
        let locale = self.host.editor_state().editor_ui.locale;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let choice = ui.ask(locale, &name, conflict);
        eprintln!(
            "[save] {} is {conflict:?}: the user chose {choice:?}",
            path.display()
        );
        match choice {
            ConflictChoice::Overwrite => SaveGuard::Proceed,
            ConflictChoice::Cancel => SaveGuard::Cancel,
            ConflictChoice::SaveCopy => {
                let default = unique_copy_path(path);
                match ui.pick_copy_path(self.host.editor_state(), &default) {
                    Some(copy) => SaveGuard::SaveCopy(copy),
                    None => SaveGuard::Cancel,
                }
            }
        }
    }

    pub(super) fn enqueue_background_save(
        &mut self,
        path: PathBuf,
        set_current_path: bool,
    ) -> EnqueueOutcome {
        self.enqueue_background_save_with(path, set_current_path, false)
    }

    fn enqueue_background_save_with(
        &mut self,
        path: PathBuf,
        set_current_path: bool,
        canonical_only: bool,
    ) -> EnqueueOutcome {
        // The clean-copy shortcut reads the source file's current bytes, so it
        // is only valid while the file is still what the editor loaded.
        let clean_source = self.current_path.clone().filter(|source| {
            set_current_path
                && !canonical_only
                && classify(self.disk_baselines.get(source), source).is_none()
        });
        let outcome = match clean_source {
            Some(source_path) => self.save_session.enqueue_clean_bound_op_save_as(
                self.host.editor_state(),
                self.host.document_epoch(),
                source_path,
                path,
                set_current_path,
                self.mcp_wake_proxy.clone(),
            ),
            None => self.save_session.enqueue(
                self.host.editor_state(),
                self.host.document_epoch(),
                path,
                set_current_path,
                self.mcp_wake_proxy.clone(),
            ),
        };
        eprintln!("[save] request {outcome:?}");
        outcome
    }
}

#[cfg(test)]
#[path = "conflict_tests.rs"]
mod tests;
