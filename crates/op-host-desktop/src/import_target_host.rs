//! Where a whole-file import (`.fig`, `.html`, `.zip`) lands.
//!
//! An import replaces the document it is installed into, so it may only be
//! installed into a pristine untitled tab (`active_document_is_pristine_untitled`).
//! Anything else gets a NEW untitled tab first and the import is installed
//! there, which leaves the previous document in its own tab untouched.
//!
//! The decision is made twice, on purpose:
//!
//! - when the import is requested (`import_may_start`): if a new tab would be
//!   needed and tabs cannot be created right now, the request is refused up
//!   front and nothing changes;
//! - when the result arrives (`pump_import_sessions`): the user may have typed
//!   or drawn in the pristine tab while the worker ran, so pristine-ness is
//!   re-checked at the last moment. The tab is opened BEFORE the result is
//!   installed; the install never runs against the old tab.
//!
//! Binding a result to its tab: while an import session is live, switching,
//! closing and opening tabs are all refused (`document_switch_blocked`), so
//! the document on screen when the result arrives is the one the import was
//! requested in. That is the whole async story; no per-result tab id is kept.

use crate::{figma_import_session, html_import_session, DesktopApp};
use figma_import_session::PumpOutcome;

impl DesktopApp {
    /// Whether an import requested now may go ahead. `false` after telling the
    /// user why, with nothing changed. Call before cancelling anything.
    pub(crate) fn import_may_start(&mut self) -> bool {
        if self.active_document_is_pristine_untitled() {
            return true;
        }
        // The import sessions are about to be superseded by this request, so
        // they do not count as a reason to refuse.
        match self.document_switch_blocked_with(true) {
            Some(reason) => {
                self.refuse_document_switch(reason);
                false
            }
            None => true,
        }
    }

    /// Open the fresh untitled tab a finished import is installed into.
    fn open_tab_for_import_result(&mut self) -> bool {
        if let Some(reason) = self.document_switch_blocked_with(true) {
            self.refuse_document_switch(reason);
            return false;
        }
        self.open_untitled_tab_unchecked()
    }

    fn pump_figma_import(&mut self) -> PumpOutcome {
        let can_replace =
            self.current_figma_import.is_none() || self.active_document_is_pristine_untitled();
        let outcome = figma_import_session::pump(
            &mut self.host,
            &mut self.current_figma_import,
            &mut self.current_path,
            self.window.as_ref(),
            can_replace,
        );
        if outcome != PumpOutcome::NeedsFreshTab {
            return outcome;
        }
        if !self.open_tab_for_import_result() {
            figma_import_session::cancel(&mut self.host, &mut self.current_figma_import);
            return PumpOutcome::Cancelled;
        }
        figma_import_session::pump(
            &mut self.host,
            &mut self.current_figma_import,
            &mut self.current_path,
            self.window.as_ref(),
            true,
        )
    }

    fn pump_html_import(&mut self) -> PumpOutcome {
        let can_replace =
            self.current_html_import.is_none() || self.active_document_is_pristine_untitled();
        let outcome = html_import_session::pump(
            &mut self.host,
            &mut self.current_html_import,
            &mut self.current_path,
            self.window.as_ref(),
            can_replace,
        );
        if outcome != PumpOutcome::NeedsFreshTab {
            return outcome;
        }
        if !self.open_tab_for_import_result() {
            html_import_session::cancel(&mut self.host, &mut self.current_html_import);
            return PumpOutcome::Cancelled;
        }
        html_import_session::pump(
            &mut self.host,
            &mut self.current_html_import,
            &mut self.current_path,
            self.window.as_ref(),
            true,
        )
    }

    /// Drain both import sessions once per frame. Returns whether the next
    /// frame must repaint.
    pub(crate) fn pump_import_sessions(&mut self) -> bool {
        let mut dirty = false;
        match self.pump_figma_import() {
            PumpOutcome::CompletedOk | PumpOutcome::CompletedSaved => {
                self.rebind_git_session_for_current_path();
                // The import installed a fresh `EditorState` whose revision
                // restarts at 0 and whose node ids can collide with the
                // replaced document's: drop all image-search state.
                self.image_search.reset();
                dirty = true;
            }
            PumpOutcome::CompletedErr | PumpOutcome::SelectionReady | PumpOutcome::Cancelled => {
                dirty = true
            }
            PumpOutcome::StillPending | PumpOutcome::Idle | PumpOutcome::NeedsFreshTab => {}
        }
        match self.pump_html_import() {
            PumpOutcome::CompletedOk | PumpOutcome::CompletedSaved => {
                self.rebind_git_session_for_current_path();
                self.image_search.reset();
                dirty = true;
            }
            PumpOutcome::CompletedErr => dirty = true,
            PumpOutcome::SelectionReady
            | PumpOutcome::Cancelled
            | PumpOutcome::Idle
            | PumpOutcome::StillPending
            | PumpOutcome::NeedsFreshTab => {}
        }
        dirty
    }
}
