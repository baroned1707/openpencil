//! Desktop half of the Studio Home document swap.
//!
//! A brief sent from Home swaps in a fresh page (op-host-native
//! `widget_host::home_document_swap`). Two things used to go wrong on
//! desktop: the replaced design's unsaved work vanished, and `current_path`
//! still named the replaced design's file, so the next ⌘S wrote the NEW
//! design over it. The shell drains the parked document here: the new design
//! starts untitled, and unsaved work stays open as its own tab (bound to its
//! own file when it had one, so Save writes it back where it came from). No
//! dialog: Home is where a new thing starts, and a prompt at that moment would
//! only be clicked through.
//!
//! The swap itself still happens inside the host, before the shell is asked,
//! so the replaced work is handed over as a snapshot and re-opened here, one
//! frame later, as a tab that stays in the background (the brief's run is
//! already queued for the document on screen). A document with nothing unsaved
//! is not kept open: its file is on disk and in the recent list.

use std::path::PathBuf;

use op_host_native::widget_host::{ParkedDocument, ReplacedHomeDocument};

use crate::recovery_store::SnapshotReason;
use crate::DesktopApp;

impl DesktopApp {
    /// Drain the document the last Home brief replaced. Returns true when
    /// state changed.
    pub(crate) fn drain_replaced_home_document(&mut self) -> bool {
        let Some(replaced) = self.host.take_replaced_home_document() else {
            return false;
        };
        let previous_path = self.current_path.take();
        crate::persistence::refresh_title(&self.current_path, self.window.as_ref());
        if replaced.had_unsaved_changes {
            self.keep_replaced_document_in_a_background_tab(replaced, previous_path);
        }
        true
    }

    /// Open the replaced, unsaved document as a tab behind the one on screen,
    /// still unsaved and bound to the file it came from.
    fn keep_replaced_document_in_a_background_tab(
        &mut self,
        replaced: ReplacedHomeDocument,
        path: Option<PathBuf>,
    ) {
        let ReplacedHomeDocument {
            mut state,
            thumbnails,
            ..
        } = replaced;
        // Home was showing when the brief was sent; the kept design opens as
        // a design, not behind the start screen.
        state.editor_ui.home.hide();
        let mut slot = self.documents.new_slot();
        let kept = slot.id;
        slot.parked = Some(ParkedDocument::from_state(*state, thumbnails));
        slot.local.path = path;
        self.documents.push_background(slot);
        // The tab on screen kept its document id, and with it the crash
        // snapshot of the work that now lives in the new tab. Hand the work
        // over only once the new tab has a snapshot of its own (the same rule
        // as recovering a document), so a crash right now loses nothing; if the
        // write fails the work is merely offered twice after a crash.
        self.snapshot_document_now(kept, SnapshotReason::Autosnapshot);
        if self.recovery.has_entry(kept) {
            let on_screen = self.documents.active_id();
            self.recovery_clear(on_screen);
        }
        self.publish_document_tabs();
    }
}

#[cfg(test)]
#[path = "home_document_swap_host_tests.rs"]
mod host_tests;
