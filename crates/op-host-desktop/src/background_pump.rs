//! Keeping every open document's background work moving while the window is
//! not repainting.
//!
//! Agent runs, image-slot enrichment and font downloads are pumped from
//! `on_redraw_requested`. A window the system does not repaint (occluded,
//! minimised) stops that, and every in-app agent would stall with it, in every
//! document. MCP avoids this by draining on `new_events`; this module gives
//! the same wake-up path to the in-app work: when a wake arrives and no redraw
//! has pumped for a while, it pumps instead.
//!
//! A redraw stamps every pump it makes, so the two paths never both run in the
//! same stretch: while the window repaints at its usual 30 Hz the wake path
//! finds a fresh stamp and does nothing.
//!
//! NOT verified on a real occluded macOS window: whether the event loop keeps
//! waking us (timers may be coalesced by App Nap) and whether `request_redraw`
//! is swallowed is the platform's call. What is tested is the decision below
//! and that a pump from the wake path advances runs and image jobs.

use crate::DesktopApp;
use std::time::{Duration, Instant};

/// A wake pumps on its own once no redraw has pumped for this long. Several
/// times the 33 ms cadence of a visible window, so a repainting window never
/// reaches it.
pub(crate) const REDRAW_STALE_AFTER: Duration = Duration::from_millis(150);

/// Whether the wake path should pump now: there is background work and the
/// last pump (by either path) is older than [`REDRAW_STALE_AFTER`], or there
/// was none yet. Pure.
pub(crate) fn wake_pump_due(last_pump: Option<Instant>, now: Instant, has_work: bool) -> bool {
    has_work && last_pump.is_none_or(|at| now.saturating_duration_since(at) >= REDRAW_STALE_AFTER)
}

impl DesktopApp {
    /// A redraw is about to pump the open documents' background work.
    pub(crate) fn note_background_pump(&mut self) {
        self.last_background_pump = Some(Instant::now());
    }

    /// Whether any document has background work a pump would advance.
    pub(crate) fn has_background_work(&self) -> bool {
        self.any_run_live() || self.any_image_search_pending() || self.font_downloads.is_busy()
    }

    /// The wake path: pump runs, image enrichment and font downloads when no
    /// redraw has. `None` when nothing was due (a redraw pumped recently, or
    /// there is no work); otherwise whether the screen needs a repaint.
    pub(crate) fn pump_background_when_redraw_starved(&mut self) -> Option<bool> {
        let now = Instant::now();
        if !wake_pump_due(self.last_background_pump, now, self.has_background_work()) {
            return None;
        }
        self.last_background_pump = Some(now);
        let mut changed = self.pump_document_runs();
        changed |= self.pump_image_enrichment();
        changed |= self.pump_font_downloads();
        Some(changed)
    }
}
