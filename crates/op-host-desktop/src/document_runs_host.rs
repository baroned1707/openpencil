//! `DesktopApp` side of per-document agent runs: pumping every document's
//! runs each frame, reaching a parked document's runs, and tearing them down.
//!
//! The on-screen document's runs are `DesktopApp::runs`; a parked document's
//! live in its registry slot. A run only ever touches the document it was
//! started in: its pump runs with that document standing in on the host (the
//! lens, `WidgetHostNative::with_parked_document`) and with that document's
//! indicator scope current.

use crate::chat_session::{self, ChatLaunchContext};
use crate::document_runs::{pump_runs, DocumentRuns};
use crate::DesktopApp;
use op_editor_core::agent_indicators;
use op_editor_core::DocumentTabStatus;
use op_host_native::WidgetHostNative;

impl DesktopApp {
    /// Point the thread's epoch-less indicator calls at the document on screen.
    /// Painters and every launch / stop path read the registry that way.
    pub(crate) fn sync_indicator_scope(&self) {
        agent_indicators::set_current_scope(self.documents.active_id().0);
    }

    /// Whether any open document, on screen or parked, has a live run.
    pub(crate) fn any_run_live(&self) -> bool {
        self.runs.is_live() || !self.documents.parked_with_live_runs().is_empty()
    }

    /// Whether the document at `index` has a live run.
    pub(crate) fn slot_has_live_run(&self, index: usize) -> bool {
        if index == self.documents.active_index() {
            self.runs.is_live()
        } else {
            self.documents
                .indices_with_live_runs(false)
                .contains(&index)
        }
    }

    /// Run `f` with the host showing the document at `index` and that
    /// document's runs. `None` when there is no such slot, or when the
    /// document is parked and collaboration forbids showing it.
    pub(crate) fn with_slot_runs<R>(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut WidgetHostNative, &mut DocumentRuns) -> R,
    ) -> Option<R> {
        let id = self.documents.id_at(index)?;
        let _scope = agent_indicators::enter_scope(id.0);
        if index == self.documents.active_index() {
            return Some(f(&mut self.host, &mut self.runs));
        }
        let (mut parked, mut runs) = self.documents.take_runs(index)?;
        let result = self
            .host
            .with_parked_document(&mut parked, |host| f(host, &mut runs));
        self.documents.return_runs(index, parked, runs);
        result
    }

    /// Pump the runs of every document once: the one on screen directly, the
    /// others through the lens. Returns whether the screen needs a repaint: a
    /// parked document's streaming does not, only a change of its status does.
    pub(crate) fn pump_document_runs(&mut self) -> bool {
        let viewport = (self.viewport_width, self.viewport_height);
        let active = self.documents.active_index();
        let mut changed = self
            .with_slot_runs(active, |host, runs| pump_runs(host, runs, viewport, true))
            .unwrap_or(false);
        for index in self.documents.parked_with_live_runs() {
            let status = self
                .with_slot_runs(index, |host, runs| {
                    pump_runs(host, runs, viewport, false);
                    runs.status
                })
                .unwrap_or(DocumentTabStatus::Running);
            changed |= status != DocumentTabStatus::Running;
        }
        self.sync_active_slot();
        changed
    }

    /// Stop every run of the document at `index` and leave its transcript in
    /// the stopped state. For a parked document the teardown runs through the
    /// lens; if collaboration forbids that, the runs are simply dropped (their
    /// workers stop and their indicator epochs end), which is all that is left
    /// to do for a document that is going away.
    pub(crate) fn cancel_runs_of_slot(&mut self, index: usize) {
        if self
            .with_slot_runs(index, |host, runs| runs.cancel(host))
            .is_some()
        {
            return;
        }
        if let Some(id) = self.documents.id_at(index) {
            if let Some(slot) = self.documents.slot_mut(index) {
                let runs = std::mem::take(&mut slot.local.runs);
                drop(runs);
            }
            agent_indicators::drop_scope(id.0);
        }
    }

    /// Best-effort finalize of the design loop of the document at `index`
    /// without stopping it, so a Save at the close prompt writes the finished
    /// design. The run itself is stopped only once the close is confirmed.
    pub(crate) fn finalize_runs_of_slot(&mut self, index: usize) {
        self.with_slot_runs(index, |host, runs| {
            chat_session::finalize_design_session_if_needed(
                host,
                &runs.current_chat,
                "teardown-backstop",
            );
        });
    }

    /// Best-effort finalize of every in-flight design loop before the process
    /// exits (see `chat_session::finalize_design_session_if_needed`).
    pub(crate) fn finalize_runs_before_exit(&mut self) {
        chat_session::finalize_design_session_if_needed(
            &mut self.host,
            &self.runs.current_chat,
            "teardown-backstop",
        );
        for index in self.documents.parked_with_live_runs() {
            self.with_slot_runs(index, |host, runs| {
                chat_session::finalize_design_session_if_needed(
                    host,
                    &runs.current_chat,
                    "teardown-backstop",
                );
            });
        }
    }

    /// Act on what the user already asked of the document that is about to
    /// leave the screen. A New chat / Stop / close-tab click or a send that is
    /// still queued would otherwise travel with the parked document and be
    /// drained later, against the document that is on screen by then.
    pub(crate) fn settle_leaving_document(&mut self) {
        self.drain_new_chat();
        self.drain_stop_chat();
        self.drain_close_chat_tab();
        self.launch_chat_if_pending();
    }

    /// The name of the document at `index` as a dialog shows it.
    pub(crate) fn slot_display_name(&mut self, index: usize) -> String {
        self.sync_active_slot();
        let locale = self.host.editor_state().editor_ui.locale;
        self.documents
            .slot_paths()
            .get(index)
            .cloned()
            .flatten()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| op_i18n::translate(locale, "dialog.untitledDocument").to_string())
    }

    /// Ask whether to stop the agent of the document at `index` so it can
    /// close. Only Yes goes ahead; a missing dialog backend keeps the tab.
    pub(crate) fn confirm_stop_runs_to_close(
        &mut self,
        index: usize,
        ask: &mut impl FnMut(&str, &str) -> Option<crate::message_dialog::Choice>,
    ) -> bool {
        let name = self.slot_display_name(index);
        self.ask_to_stop_runs(&name, ask)
            .is_some_and(|choice| choice == crate::message_dialog::Choice::Yes)
    }

    /// Quit asks the same question for every document with a live run. A
    /// missing dialog backend lets the quit through: it must never be
    /// swallowed (#197).
    pub(crate) fn confirm_stop_runs_to_quit(
        &mut self,
        ask: &mut impl FnMut(&str, &str) -> Option<crate::message_dialog::Choice>,
    ) -> bool {
        self.sync_active_slot();
        let live = self.documents.indices_with_live_runs(self.runs.is_live());
        if live.is_empty() {
            return true;
        }
        let names: Vec<String> = live
            .into_iter()
            .map(|index| self.slot_display_name(index))
            .collect();
        match self.ask_to_stop_runs(&names.join(", "), ask) {
            Some(choice) => choice == crate::message_dialog::Choice::Yes,
            None => true,
        }
    }

    fn ask_to_stop_runs(
        &self,
        name: &str,
        ask: &mut impl FnMut(&str, &str) -> Option<crate::message_dialog::Choice>,
    ) -> Option<crate::message_dialog::Choice> {
        let locale = self.host.editor_state().editor_ui.locale;
        let body = op_i18n::translate(locale, "dialog.agentRunningBody").replace("{{name}}", name);
        ask(
            op_i18n::translate(locale, "dialog.agentRunningTitle"),
            &body,
        )
    }

    /// Stop every run of every open document (the user agreed to quit).
    pub(crate) fn cancel_all_runs(&mut self) {
        for index in 0..self.documents.len() {
            if self.slot_has_live_run(index) {
                self.cancel_runs_of_slot(index);
            }
        }
    }

    /// Launch-time facts about the document on screen: agents started from it
    /// work in ITS project root, not in whichever document was open before, and
    /// reach the canvas through ITS MCP endpoint so a tab switch cannot
    /// redirect them.
    pub(crate) fn launch_context(&self) -> ChatLaunchContext {
        ChatLaunchContext::for_document(self.current_path.as_deref())
            .with_document(Some(self.documents.active_id()))
    }
}
