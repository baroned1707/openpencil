//! Save a document to its own file when an agent run on it ends.
//!
//! Agent edits live in memory until somebody saves. When the LAST live run of
//! a document ends successfully, the editor saves that document in place (the
//! same routine as the MCP `save_document` without a `filePath`, see
//! `op_host_services::save_in_place`) so finished agent work does not wait for
//! Cmd+S. The decision is the pure [`should_autosave_after_run`]; the host
//! half below gathers its facts, runs it on the right document (on screen, or
//! parked through the lens without switching tabs) and reports a failure.
//!
//! The setting `autosave_on_agent_run_end` (default on) is the switch "Save
//! automatically when an agent finishes" in Settings > System.

use op_editor_core::editor_toast::EditorToastLevel;
use op_editor_core::{CollabEditSource, CollabGateAction, CollabGatePolicy, DocumentId};
use op_host_services::save_in_place::{save_in_place_unless_busy, SaveInPlaceError};

use crate::DesktopApp;

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunEnd {
    /// The turn finished and its transcript reports no error.
    Succeeded,
    /// The turn ended in an error (a timeout reads as one).
    Failed,
    /// The user stopped it, or its document was closed: partial work.
    Cancelled,
}

/// Everything the decision depends on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AutosaveFacts {
    /// `autosave_on_agent_run_end`.
    pub setting_enabled: bool,
    pub run_end: RunEnd,
    /// Another run of the same document is still going.
    pub other_run_live: bool,
    pub document_dirty: bool,
    /// The document has a file.
    pub bound_to_file: bool,
    /// A collaboration session lets this document be saved by an agent.
    pub collaboration_allows_save: bool,
    /// The user has a File-menu action or tab request queued and not yet
    /// handled: their own save or close comes first.
    pub user_request_pending: bool,
    /// A Cmd+S / Save As write is still running.
    pub save_in_flight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkipReason {
    SettingOff,
    RunNotSuccessful,
    NotLastRun,
    Untitled,
    NothingToSave,
    Collaboration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferReason {
    UserRequestPending,
    SaveInFlight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutosaveDecision {
    Save,
    /// Not now: try again on a later frame.
    Defer(DeferReason),
    /// Never for this run.
    Skip(SkipReason),
}

/// Whether to save a document right now, in the order the reasons matter: a
/// disabled setting or a run that did not succeed ends the question, and so
/// does a document that has no file, nothing to save or a collaboration
/// session; only a busy editor postpones it.
pub(crate) fn should_autosave_after_run(facts: &AutosaveFacts) -> AutosaveDecision {
    use AutosaveDecision::{Defer, Save, Skip};
    if !facts.setting_enabled {
        return Skip(SkipReason::SettingOff);
    }
    if facts.run_end != RunEnd::Succeeded {
        return Skip(SkipReason::RunNotSuccessful);
    }
    if facts.other_run_live {
        return Skip(SkipReason::NotLastRun);
    }
    if !facts.bound_to_file {
        return Skip(SkipReason::Untitled);
    }
    if !facts.document_dirty {
        return Skip(SkipReason::NothingToSave);
    }
    if !facts.collaboration_allows_save {
        return Skip(SkipReason::Collaboration);
    }
    if facts.user_request_pending {
        return Defer(DeferReason::UserRequestPending);
    }
    if facts.save_in_flight {
        return Defer(DeferReason::SaveInFlight);
    }
    Save
}

/// A run end waiting for the editor to be free.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PendingAutosave {
    pub id: DocumentId,
    end: RunEnd,
    attempts: u32,
}

/// Frames a deferred autosave is retried before it is reported as failed.
const MAX_DEFER_ATTEMPTS: u32 = 600;

impl DesktopApp {
    /// Run the autosave decision for every document whose last run just ended
    /// (`ended`) and every one still waiting for a free editor. Call it once
    /// per frame, right after the runs were pumped. Returns whether the screen
    /// needs a repaint.
    pub(crate) fn autosave_after_runs(&mut self, ended: Vec<(DocumentId, RunEnd)>) -> bool {
        let mut changed = false;
        let mut waiting = std::mem::take(&mut self.autosave_deferred);
        for (id, end) in ended {
            waiting.retain(|pending| pending.id != id);
            waiting.push(PendingAutosave {
                id,
                end,
                attempts: 0,
            });
        }
        if waiting.is_empty() {
            return false;
        }
        self.sweep_disk_baselines();
        for mut pending in waiting {
            match self.try_autosave(pending.id, pending.end) {
                AutosaveAttempt::Done => changed = true,
                AutosaveAttempt::Retry => {
                    pending.attempts += 1;
                    if pending.attempts < MAX_DEFER_ATTEMPTS {
                        self.autosave_deferred.push(pending);
                    } else {
                        eprintln!(
                            "[autosave] gave up on {:?}: the editor stayed busy",
                            pending.id
                        );
                        self.toast_autosave_failed();
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    fn try_autosave(&mut self, id: DocumentId, run_end: RunEnd) -> AutosaveAttempt {
        let Some(index) = self.documents.index_of(id) else {
            return AutosaveAttempt::Done; // closed meanwhile: nothing to save
        };
        self.sync_active_slot();
        let path = self.documents.slot_paths().get(index).cloned().flatten();
        let baseline = path
            .as_deref()
            .and_then(|path| self.disk_baselines.get(path));
        let editor_ui = &self.host.editor_state().editor_ui;
        let setting_enabled = editor_ui.agent_settings.autosave_on_agent_run_end;
        let user_request_pending =
            editor_ui.pending_file_action.is_some() || editor_ui.pending_document_request.is_some();
        let save_in_flight = self.save_session.is_active();
        let attempt = self.with_slot_runs(index, |host, runs| {
            let state = host.editor_state_mut();
            let facts = AutosaveFacts {
                setting_enabled,
                run_end,
                other_run_live: runs.is_live(),
                document_dirty: state.is_dirty(),
                bound_to_file: path.is_some(),
                collaboration_allows_save: CollabGatePolicy::from(&state.editor_ui.collab)
                    .check(CollabGateAction::SaveShared, CollabEditSource::Mcp)
                    .is_ok(),
                user_request_pending,
                save_in_flight,
            };
            let decision = should_autosave_after_run(&facts);
            let saved = (decision == AutosaveDecision::Save).then(|| {
                save_in_place_unless_busy(state, path.as_deref(), baseline.as_ref(), save_in_flight)
            });
            (decision, saved)
        });
        match attempt {
            // A parked document the host cannot show (collaboration): leave it.
            None => AutosaveAttempt::Done,
            Some((AutosaveDecision::Defer(_), _)) => AutosaveAttempt::Retry,
            Some((AutosaveDecision::Skip(reason), _)) => {
                eprintln!("[autosave] skipped {id:?}: {reason:?}");
                AutosaveAttempt::Done
            }
            Some((AutosaveDecision::Save, Some(Ok(saved)))) => {
                self.finish_in_place_save(id, &saved);
                AutosaveAttempt::Done
            }
            Some((AutosaveDecision::Save, Some(Err(error)))) if error.is_transient() => {
                AutosaveAttempt::Retry
            }
            Some((AutosaveDecision::Save, Some(Err(error)))) => {
                self.report_autosave_failure(id, &error);
                AutosaveAttempt::Done
            }
            Some((AutosaveDecision::Save, None)) => AutosaveAttempt::Done,
        }
    }

    /// A failed autosave is never silent: the log has the reason, the toast
    /// tells the user, and the document stays dirty for a manual save.
    fn report_autosave_failure(&mut self, id: DocumentId, error: &SaveInPlaceError) {
        eprintln!("[autosave] {id:?}: {error}");
        self.toast_autosave_failed();
    }

    /// Reuses the existing "Save failed" string (`dialog.saveErrorTitle`); the
    /// reason is in the log.
    fn toast_autosave_failed(&mut self) {
        let now_ms = self.host.now_ms();
        self.host.editor_state_mut().editor_ui.show_toast(
            "dialog.saveErrorTitle",
            Vec::new(),
            EditorToastLevel::Warn,
            now_ms,
        );
        self.host.mark_editor_state_dirty();
    }
}

enum AutosaveAttempt {
    Done,
    Retry,
}

// Tests live in `agent_run_autosave_tests.rs`, nested in `document_runs_tests`
// to reuse its fake-run fixtures.
