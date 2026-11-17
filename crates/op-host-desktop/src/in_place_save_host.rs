//! What the desktop does after a document was saved to its own file by an
//! agent (MCP `save_document` without a `filePath`) or by the autosave at the
//! end of a run: the same follow-ups a completed Cmd+S applies.

use op_editor_core::DocumentId;
use op_host_services::save_in_place::SavedInPlace;

use crate::DesktopApp;

impl DesktopApp {
    /// `saved` was written and its document marked saved by the caller. Record
    /// the file's new on-disk state as the baseline the next in-place save is
    /// checked against, list the file as a recent one, drop the document's
    /// crash-recovery snapshot (the file holds that content now) and refresh
    /// the tab row so the unsaved dot goes out.
    pub(crate) fn finish_in_place_save(&mut self, id: DocumentId, saved: &SavedInPlace) {
        eprintln!(
            "[save] saved {id:?} in place: {} ({} bytes)",
            saved.path.display(),
            saved.bytes
        );
        self.disk_baselines.record(&saved.path, saved.fingerprint);
        crate::settings_io::touch_recent(&mut self.host, &saved.path);
        self.publish_document_tabs();
        self.clear_obsolete_recovery_entries();
        self.host.mark_editor_state_dirty();
    }
}
