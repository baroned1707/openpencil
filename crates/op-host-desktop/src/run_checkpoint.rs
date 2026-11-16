//! "Revert Last Agent Run": put the document on screen back to how it was when
//! its most recent agent run started.
//!
//! Each document remembers ONE [`RunCheckpoint`], in its [`DocumentRuns`]
//! (memory only; the crash-recovery store is a different safety net with a
//! different job). It is captured on the UI thread right before a run launches,
//! because the launch itself may already edit the document, and it is kept only
//! if the run really started. A new run replaces it; reverting, closing the
//! tab or replacing the document drops it.
//!
//! A checkpoint is an `EditorSnapshot`, the same value undo history stores:
//! top-level nodes are shared by `Arc` with the history and image payloads are
//! `Arc<str>`, so holding one costs the changed nodes, never the images. The
//! revert installs it through `EditorState::revert_to_snapshot_with_undo`, so
//! the revert is one Cmd+Z step and the user's earlier undo steps stay.

use crate::document_runs::DocumentRuns;
use crate::message_dialog::Choice;
use crate::DesktopApp;
use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{CollabEditSource, CollabGateAction, EditorSnapshot};
use op_host_native::WidgetHostNative;

/// How the document looked right before an agent run launched.
pub(crate) struct RunCheckpoint {
    snapshot: EditorSnapshot,
    /// `WidgetHostNative::document_epoch` of the document it was taken from.
    /// A whole-document install (open, import, new) gives the host a new one.
    epoch: u64,
    /// `EditorState::document_generation`. An in-place whole-document replace
    /// (MCP `ReplaceDocument`) keeps the epoch but bumps this.
    generation: u64,
}

impl RunCheckpoint {
    /// Capture the document `host` is showing.
    pub(crate) fn capture(host: &WidgetHostNative) -> Self {
        let state = host.editor_state();
        Self {
            snapshot: state.snapshot_for_history(),
            epoch: host.document_epoch(),
            generation: state.document_generation(),
        }
    }

    /// Whether the launch that followed this capture touched the document (it
    /// can, before any session exists: a design brief clears the starter frame).
    fn document_changed_since(&self, host: &WidgetHostNative) -> bool {
        host.editor_state().document_revision() != self.snapshot.revision
    }

    /// Whether the document `host` is showing is still the one this was taken
    /// from (not replaced since).
    fn is_for(&self, host: &WidgetHostNative) -> bool {
        self.epoch == host.document_epoch()
            && self.generation == host.editor_state().document_generation()
    }

    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> &EditorSnapshot {
        &self.snapshot
    }
}

impl DocumentRuns {
    /// Forget the checkpoint when the document `host` shows is no longer the
    /// one it was taken from. Called every frame by the pump.
    pub(crate) fn drop_stale_checkpoint(&mut self, host: &WidgetHostNative) {
        if self
            .checkpoint
            .as_ref()
            .is_some_and(|checkpoint| !checkpoint.is_for(host))
        {
            self.checkpoint = None;
        }
    }
}

/// Why a revert did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevertError {
    /// No agent run has started in this document (or it was already reverted).
    NothingToRevert,
    /// The run is still going. The user stops it first; it is never stopped
    /// for them.
    RunIsLive,
    /// The document was replaced after the run started, so the checkpoint
    /// belongs to a document that is gone.
    DocumentReplaced,
    /// A collaboration session does not allow replacing the document. The
    /// host has already told the user why.
    CollaborationActive,
}

impl RevertError {
    /// Locale key of the toast that explains the refusal, if the host has not
    /// already shown a notice of its own.
    fn toast_key(self) -> Option<&'static str> {
        match self {
            Self::NothingToRevert => Some("menu.revertAgentRun.nothing"),
            Self::RunIsLive => Some("menu.revertAgentRun.runLive"),
            Self::DocumentReplaced => Some("menu.revertAgentRun.replaced"),
            Self::CollaborationActive => None,
        }
    }
}

impl std::fmt::Display for RevertError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NothingToRevert => "no agent run to revert in this document",
            Self::RunIsLive => "the agent run is still live; stop it first",
            Self::DocumentReplaced => "the document was replaced after the run started",
            Self::CollaborationActive => "a collaboration session does not allow the revert",
        })
    }
}

impl std::error::Error for RevertError {}

/// What a revert that was allowed to ask did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevertOutcome {
    Reverted,
    /// The user answered No (or the question could not be shown): nothing
    /// changed and the checkpoint is kept.
    Declined,
}

impl DesktopApp {
    /// Whether "Revert Last Agent Run" would act on the document on screen:
    /// it has a checkpoint of a run that has ended, and the document is still
    /// the one the checkpoint was taken from.
    pub(crate) fn has_revertable_run(&self) -> bool {
        self.check_revertable().is_ok()
    }

    fn check_revertable(&self) -> Result<&RunCheckpoint, RevertError> {
        let checkpoint = self
            .runs
            .checkpoint
            .as_ref()
            .ok_or(RevertError::NothingToRevert)?;
        if !checkpoint.is_for(&self.host) {
            return Err(RevertError::DocumentReplaced);
        }
        if self.runs.is_live() {
            return Err(RevertError::RunIsLive);
        }
        Ok(checkpoint)
    }

    /// The one hook every launch path calls before it starts a run. When
    /// `run_is_waiting` says a run is about to launch, take the checkpoint
    /// (before the launch edits anything) and write the pre-run recovery
    /// snapshot; the caller installs the checkpoint if the run really starts.
    pub(crate) fn prepare_run_start(
        &mut self,
        run_is_waiting: impl FnOnce(&op_editor_core::ChatSessions) -> bool,
    ) -> Option<RunCheckpoint> {
        if !run_is_waiting(&self.host.editor_state().chat) {
            return None;
        }
        let checkpoint = RunCheckpoint::capture(&self.host);
        self.snapshot_before_run();
        Some(checkpoint)
    }

    /// Decide whether the launch that just returned started a run, and if so
    /// keep `candidate` (taken before the launch) as this document's checkpoint.
    ///
    /// `launched` alone is not enough: a send that ends in an error bubble
    /// ("no transport could be built") also reports a launch, yet changed
    /// nothing, and must not replace an earlier run's checkpoint. A run counts
    /// when a session is live, or when the launch already edited the document.
    pub(crate) fn settle_run_checkpoint(
        &mut self,
        candidate: Option<RunCheckpoint>,
        launched: bool,
    ) {
        let Some(candidate) = candidate else { return };
        if launched && (self.runs.is_live() || candidate.document_changed_since(&self.host)) {
            self.runs.checkpoint = Some(candidate);
            self.refresh_revert_menu();
        }
    }

    /// The menu action: revert the document on screen after one confirmation.
    pub(crate) fn revert_last_agent_run(&mut self) -> Result<RevertOutcome, RevertError> {
        self.revert_last_agent_run_with(|title, body| {
            crate::message_dialog::ask_yes_no(title, body, rfd::MessageLevel::Warning)
        })
    }

    /// [`Self::revert_last_agent_run`] with the question asked through `ask`.
    /// Refusals are checked before anything is asked, and are shown as a toast.
    pub(crate) fn revert_last_agent_run_with(
        &mut self,
        mut ask: impl FnMut(&str, &str) -> Option<Choice>,
    ) -> Result<RevertOutcome, RevertError> {
        let outcome = self.try_revert(&mut ask);
        if let Err(error) = outcome {
            self.refuse_revert(error);
        }
        outcome
    }

    fn try_revert(
        &mut self,
        ask: &mut impl FnMut(&str, &str) -> Option<Choice>,
    ) -> Result<RevertOutcome, RevertError> {
        self.check_revertable()?;
        if !self
            .host
            .gate_collaboration_action(CollabGateAction::ReplaceDocument, CollabEditSource::User)
        {
            return Err(RevertError::CollaborationActive);
        }
        let locale = self.host.editor_state().editor_ui.locale;
        let answer = ask(
            op_i18n::translate(locale, "dialog.revertAgentRunTitle"),
            op_i18n::translate(locale, "dialog.revertAgentRunBody"),
        );
        if answer != Some(Choice::Yes) {
            return Ok(RevertOutcome::Declined);
        }
        // The dialog ran a nested event loop; look again before acting.
        self.check_revertable()?;
        // An edit still in a text field must reach the document first, so the
        // undo step that brings the user's work back includes it.
        self.host.commit_pending_input_pub();
        let Some(checkpoint) = self.runs.checkpoint.take() else {
            return Err(RevertError::NothingToRevert);
        };
        self.host
            .editor_state_mut()
            .revert_to_snapshot_with_undo(&checkpoint.snapshot);
        self.finish_revert();
        Ok(RevertOutcome::Reverted)
    }

    /// Everything a whole-content install does besides the state change:
    /// repaint, rotate the caches keyed on node ids, show the tab as idle, and
    /// write a recovery snapshot of the reverted (unsaved) document.
    fn finish_revert(&mut self) {
        self.host.mark_editor_state_dirty();
        // Node ids of the replaced content may name different nodes now.
        self.host.force_rotate_layer_panel_owner();
        self.image_search.reset();
        self.last_painted_page = None;
        self.runs.status = op_editor_core::DocumentTabStatus::Idle;
        self.sync_active_slot();
        let id = self.documents.active_id();
        self.snapshot_document_now(id, crate::recovery_store::SnapshotReason::Autosnapshot);
        self.publish_document_tabs();
        self.refresh_revert_menu();
        self.request_redraw(true);
    }

    fn refuse_revert(&mut self, error: RevertError) {
        eprintln!("[revert] refused: {error}");
        let Some(key) = error.toast_key() else {
            return;
        };
        if error == RevertError::DocumentReplaced {
            // It can never apply again; do not keep the old content alive.
            self.runs.checkpoint = None;
        }
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            key,
            Vec::new(),
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
        self.request_redraw(true);
    }
}

#[cfg(test)]
#[path = "run_checkpoint_tests.rs"]
mod tests;
