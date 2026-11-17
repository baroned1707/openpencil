//! Keeping the Projects dock in step with the disk: WHEN a rescan runs.
//!
//! The dock shows what the last scan found, so a file created, renamed or
//! deleted outside the editor only appears after a rescan. Three things start
//! one, and nothing else does:
//!
//! 1. the window regains focus ([`DesktopApp::on_window_focus_changed`]);
//! 2. the app itself changed a file (rename, create, move to the Trash, `.fig`
//!    conversion, a tab saved to a new path): [`DesktopApp::rescan_after_own_operation`];
//! 3. a background tick, about every five seconds, ONLY while the window is
//!    focused and the dock is showing its list ([`DesktopApp::projects_background_active`]).
//!
//! An unfocused or hidden-dock app does no file-system work for the dock at all
//! (the scheduler's `poll` never asks the worker to start). The tick needs the
//! event loop to wake: [`DesktopApp::next_projects_wake`] tells it when.

use std::time::{Duration, Instant};

use crate::DesktopApp;

/// A wake the scheduler could not satisfy (nothing started) is retried no
/// sooner than this, so a due time in the past can never spin the event loop.
const MIN_WAKE_GAP: Duration = Duration::from_millis(250);

impl DesktopApp {
    /// A file changed because of something this app did: rescan every project
    /// at the next chance. One call for every kind of in-app file operation.
    pub(crate) fn rescan_after_own_operation(&mut self) {
        self.projects_host.scan.request_rescan_all();
        self.projects_host.published_key = None;
    }

    /// The window gained or lost focus. Regaining it rescans (the user may have
    /// been changing files in Finder or a terminal) when the dock is showing.
    pub(crate) fn on_window_focus_changed(&mut self, focused: bool) {
        self.projects_host.focused = Some(focused);
        if focused && self.projects_dock_visible() {
            self.rescan_after_own_operation();
            self.request_redraw(false);
        }
    }

    /// Whether the window has focus: the last focus event, else (before any
    /// event, or without a window in a headless test) what the window says.
    fn window_focused(&self) -> bool {
        self.projects_host
            .focused
            .unwrap_or_else(|| self.window.as_ref().is_none_or(|window| window.has_focus()))
    }

    /// The dock is showing its project list (not folded to its strip, not
    /// hidden behind Home or a presentation).
    pub(crate) fn projects_dock_visible(&self) -> bool {
        let ui = &self.host.editor_state().editor_ui;
        ui.projects_dock_available() && ui.projects_panel.is_open() && !ui.home.visible
    }

    /// Whether a background rescan is wanted right now.
    pub(crate) fn projects_background_active(&self) -> bool {
        self.window_focused() && self.projects_dock_visible()
    }

    /// When the event loop should wake for the next background rescan, or
    /// `None` when none is wanted (unfocused, dock folded, nothing to list, a
    /// scan already running).
    pub(crate) fn next_projects_wake(&self) -> Option<Instant> {
        if !self.projects_background_active() || self.projects_host.known.is_empty() {
            return None;
        }
        let due = self.projects_host.scan.next_background_due()?;
        Some(due.max(Instant::now() + MIN_WAKE_GAP))
    }
}

#[cfg(test)]
#[path = "projects_host_refresh_tests.rs"]
mod tests;
