//! The agent runs that belong to one open document.
//!
//! A run (a chat turn, a design turn, a team of sub-agents) streams into the
//! document it was started in, whichever document is on screen. Each document
//! therefore owns a [`DocumentRuns`]: the sessions, the chat tab they write
//! to, their canvas indicators and the status the tab strip shows. The
//! on-screen document's runs are `DesktopApp::runs`; parked documents keep
//! theirs in their registry slot, and the host pumps every document's runs each
//! frame ([`pump_runs`]), reaching a parked document through
//! `WidgetHostNative::with_parked_document`.

use crate::agent_run_autosave::RunEnd;
use crate::chat_session;
use crate::design_loop_indicator::{
    pump_design_session_indicator, pump_indicator, DesignLoopIndicator,
};
use crate::design_session::{self, DesignSession};
use crate::sub_agent_session::{self, SubAgentSession};
use op_editor_core::agent_indicators;
use op_editor_core::DocumentTabStatus;
use op_editor_host_core::chat::ChatSession;
use op_host_native::WidgetHostNative;

/// The runs of one document. `Default` is "nothing running".
#[derive(Default)]
pub(crate) struct DocumentRuns {
    /// In-flight AI chat turn. `chat.begin_send` raises `chat.pending_send`;
    /// the launcher drains it into a session here and the pump streams deltas
    /// into the transcript.
    pub current_chat: Option<ChatSession>,
    /// In-flight design-orchestrator turn. CLI standard-mode turns park both
    /// sessions while the async classifier resolves; the route not taken
    /// retires through its pump.
    pub current_design: Option<DesignSession>,
    /// Chat tab the sessions write to, captured when the turn launched. The
    /// pumps target it even after the user switches chat tabs. `None` when no
    /// run is in flight.
    pub chat_running_tab: Option<usize>,
    /// Sub-agent design loops launched by `spawn_agents`, pumped one at a time.
    pub sub_agents: Vec<SubAgentSession>,
    /// Index of the sub-agent being pumped.
    pub active_sub_agent: usize,
    /// Canvas indicator of the design-agent tool-loop.
    pub design_loop_indicator: Option<DesignLoopIndicator>,
    /// Canvas indicator of the design orchestrator.
    pub design_session_indicator: Option<DesignLoopIndicator>,
    /// What the tab strip and the Projects dock show for this document. For
    /// the on-screen document the registry holds a mirror (see
    /// `DesktopApp::sync_active_slot`).
    pub status: DocumentTabStatus,
    /// How the document looked right before its most recent run launched, for
    /// Edit > Revert Last Agent Run. Survives the run (and a Stop); see
    /// `run_checkpoint`.
    pub checkpoint: Option<crate::run_checkpoint::RunCheckpoint>,
    /// How the last run ended, until the host takes it (once per frame) to
    /// decide on an autosave (`agent_run_autosave`).
    pub run_end: Option<RunEnd>,
    /// Image-slot enrichment of THIS document: stock searches and generations
    /// that fill its empty image nodes. Per document because node ids,
    /// revisions and the scan gate restart in every document; it travels with
    /// the document on a switch, is pumped through the lens while the
    /// document is parked, and is dropped with it when the tab closes.
    pub image_search: crate::image_search_session::ImageSearchSession,
}

impl DocumentRuns {
    /// Whether any session of this document is still running.
    pub fn is_live(&self) -> bool {
        self.current_chat.is_some() || self.current_design.is_some() || !self.sub_agents.is_empty()
    }

    /// How the run that just ended finished, once.
    pub fn take_run_end(&mut self) -> Option<RunEnd> {
        self.run_end.take()
    }

    /// The user has looked at the document: a finished run no longer asks for
    /// attention. A running one keeps its status.
    pub fn mark_seen(&mut self) {
        if matches!(
            self.status,
            DocumentTabStatus::NeedsReview | DocumentTabStatus::Done | DocumentTabStatus::Error
        ) {
            self.status = DocumentTabStatus::Idle;
        }
    }

    /// Update `status` after a pump. `was_live` is whether a run was live when
    /// the pump began, `on_screen` whether this document is the one showing,
    /// `failed` whether the run ended in an error.
    fn settle_status(&mut self, was_live: bool, on_screen: bool, failed: bool) {
        self.status = if self.is_live() {
            DocumentTabStatus::Running
        } else if was_live {
            match (failed, on_screen) {
                (true, _) => DocumentTabStatus::Error,
                (false, true) => DocumentTabStatus::Done,
                (false, false) => DocumentTabStatus::NeedsReview,
            }
        } else if self.status == DocumentTabStatus::Running {
            // Stopped or retired between pumps: nothing to review.
            DocumentTabStatus::Idle
        } else {
            self.status
        };
    }

    /// Stop every run of this document. The document must be the one on the
    /// host (directly or through the lens): teardown finalizes the design and
    /// writes the stopped state into its transcript.
    pub fn cancel(&mut self, host: &mut WidgetHostNative) {
        chat_session::finalize_design_session_if_needed(
            host,
            &self.current_chat,
            "teardown-backstop",
        );
        if let Some(session) = self.current_design.as_ref() {
            session.abort();
            design_session::stop_design_transcript(host, self.chat_running_tab);
        }
        self.current_chat = None;
        self.current_design = None;
        sub_agent_session::abort_all(&mut self.sub_agents, &mut self.active_sub_agent);
        for indicator in [
            self.design_loop_indicator.take(),
            self.design_session_indicator.take(),
        ]
        .into_iter()
        .flatten()
        {
            agent_indicators::end_if_epoch(indicator.epoch);
        }
        if let Some(epoch) = agent_indicators::active_epoch() {
            agent_indicators::end_if_epoch(epoch);
        }
        let running_tab = self.chat_running_tab.take();
        let state = host.editor_state_mut();
        let chat = state.chat.run_tab_mut(running_tab);
        chat.agents_running = (0, 0);
        chat.pending_send = None;
        for message in &mut chat.messages {
            message.streaming = false;
        }
        host.mark_editor_state_dirty();
        self.status = DocumentTabStatus::Idle;
        // Partial work: never saved on the strength of this run.
        self.run_end = Some(RunEnd::Cancelled);
    }
}

/// Pump one document's runs. `host` must be showing that document (the caller
/// is either on-screen or inside the lens) and the indicator scope must be its
/// own. Returns whether anything changed.
///
/// Every step reads and writes the document through `host.editor_state_mut()`
/// and the registry through the thread's current indicator scope, so one run
/// never reaches another document.
pub(crate) fn pump_runs(
    host: &mut WidgetHostNative,
    runs: &mut DocumentRuns,
    viewport: (f32, f32),
    on_screen: bool,
) -> bool {
    let was_live = runs.is_live();
    let mut changed = false;
    runs.drop_stale_checkpoint(host);
    // Tell the `spawn_agents` ack whether THIS document's batch is running, so
    // a second call is refused instead of acked and then dropped below.
    sub_agent_session::set_sub_batch_live(!runs.sub_agents.is_empty());
    // Deltas land in the tab the turn was bound to, not the active tab.
    changed |= chat_session::pump(
        host,
        &mut runs.current_chat,
        runs.chat_running_tab,
        None,
        viewport,
    );
    // Frame glows and the N/M header of the design-agent loop.
    pump_indicator(
        &mut runs.design_loop_indicator,
        &runs.current_chat,
        host.editor_state_mut(),
    );
    // Sub-agents the parent loop just stashed through `spawn_agents`. The stash
    // is process-wide, so it is taken right after the pump that may have made
    // it, before any other document is pumped. A second `spawn_agents` while a
    // batch is live is dropped (overwriting would leak the batch's epoch).
    if let Some(specs) = sub_agent_session::take_pending_spawn() {
        if runs.sub_agents.is_empty() {
            runs.sub_agents = sub_agent_session::launch_sub_agents(host, specs);
            runs.active_sub_agent = 0;
            changed |= !runs.sub_agents.is_empty();
        }
    }
    changed |= sub_agent_session::pump_sub_agents(
        host,
        &mut runs.sub_agents,
        &mut runs.active_sub_agent,
        runs.chat_running_tab,
    );
    // Orchestrator apply requests and progress, then its indicator, so a
    // same-frame turn finish is observed.
    changed |=
        design_session::pump_commands(host, &mut runs.current_design, viewport.0, viewport.1);
    changed |= design_session::pump_progress(host, &mut runs.current_design, runs.chat_running_tab);
    pump_design_session_indicator(
        &mut runs.design_session_indicator,
        &runs.current_design,
        host.editor_state_mut(),
        runs.chat_running_tab,
    );
    // Each pump retires its session when the turn finishes; once nothing is in
    // flight the tab binding is stale (a fresh turn captures the active tab).
    let finished = was_live && !runs.is_live();
    let failed = finished && run_tab_failed(host.editor_state(), runs.chat_running_tab);
    if finished {
        runs.run_end = Some(if failed {
            RunEnd::Failed
        } else {
            RunEnd::Succeeded
        });
    }
    if !runs.is_live() {
        runs.chat_running_tab = None;
    }
    let before = runs.status;
    runs.settle_status(was_live, on_screen, failed);
    changed || runs.status != before
}

/// Whether the run bound to `running_tab` ended in an error, judged on that
/// tab's last assistant message (not the active tab's).
fn run_tab_failed(state: &op_editor_core::EditorState, running_tab: Option<usize>) -> bool {
    let tab = running_tab
        .and_then(|index| state.chat.tabs().get(index))
        .unwrap_or_else(|| state.chat.active());
    tab.messages
        .iter()
        .rev()
        .find(|message| message.role == op_editor_core::ChatRole::Assistant)
        .is_some_and(op_editor_core::workspace_run::assistant_message_failed)
}

#[cfg(test)]
#[path = "document_runs_tests.rs"]
mod tests;
