//! Quitting without losing unsaved work: the exit snapshot and the MCP
//! `openpencil/shutdown` request (`op stop`).
//!
//! Two kinds of exit reach `ApplicationHandler::exiting`:
//!
//! * a quit the user confirmed (Quit, the window's close button, `op stop`):
//!   `confirm_close_with` asked about every unsaved tab, so each was saved or
//!   discarded on purpose and nothing of this run is worth recovering. The one
//!   exception is a document that is unsaved again after its prompt (stopping
//!   an agent finalizes its design afterwards): it is snapshotted;
//! * everything else: a Dock or logout quit on macOS (`applicationWillTerminate`
//!   ends in `exiting` without ever asking), the event loop stopping on an
//!   error. The unsaved tabs would be lost, so each is snapshotted now.

use crate::message_dialog::Choice;
use crate::recovery_store::SnapshotReason;
use crate::DesktopApp;

impl DesktopApp {
    /// Run once as the process exits (see the module docs). Idempotent: the
    /// platform may deliver `exiting` more than once.
    pub(crate) fn recovery_on_exit(&mut self) {
        if self.recovery.store().is_none() || self.recovery.exit_done {
            return;
        }
        self.recovery.exit_done = true;
        if self.recovery.quit_confirmed {
            self.recovery_exit_after_confirmed_quit();
            return;
        }
        // Land a half-typed property or text edit in the document first, so the
        // snapshot holds what the user sees.
        self.host.commit_pending_input_pub();
        self.host.editor_state_mut().text_edit_commit();
        self.snapshot_dirty_documents(SnapshotReason::Exit);
    }

    /// Whether the editor should quit because a token-authed
    /// `openpencil/shutdown` (`op stop`) arrived, asking through real dialogs.
    pub(crate) fn mcp_quit_confirmed(&mut self) -> bool {
        self.mcp_quit_confirmed_with(|title, body| {
            crate::message_dialog::ask_yes_no_cancel(title, body, rfd::MessageLevel::Warning)
        })
    }

    /// [`Self::mcp_quit_confirmed`] with the questions asked through `ask`.
    ///
    /// The request is consumed, so a quit the user cancels is dropped and the
    /// editor keeps running (the client already got its acknowledgement). A
    /// confirmed one has been through the same unsaved-changes prompts as the
    /// Quit menu item.
    pub(crate) fn mcp_quit_confirmed_with(
        &mut self,
        ask: impl FnMut(&str, &str) -> Option<Choice>,
    ) -> bool {
        if !self.take_mcp_shutdown_request() {
            return false;
        }
        // Finalize-lifecycle invariant (0718-1-k3-1 postmortem) — see
        // `chat_session::finalize_design_session_if_needed`'s doc comment.
        self.finalize_runs_before_exit();
        if !self.confirm_close_with(ask) {
            eprintln!("[mcp] shutdown request dropped: the quit was cancelled");
            // `op stop` removes the discovery file a moment after the server
            // acknowledged the request, assuming the editor is quitting. It is
            // not: publish the port again so `op` still finds this editor.
            self.publish_live_mcp_port();
            return false;
        }
        self.host.flush_settings_input();
        op_host_services::settings_io::save(self.host.editor_state());
        true
    }
}

#[cfg(test)]
#[path = "recovery_lifecycle_tests.rs"]
mod tests;
