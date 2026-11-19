//! `DesktopApp` side of image-slot enrichment: filling the empty image nodes
//! of EVERY open document, not only the one on screen.
//!
//! Each document owns an `ImageSearchSession` (`DocumentRuns::image_search`).
//! The on-screen document's session runs every frame against the host's
//! current scene. A parked document's session is reached through the host's
//! lens, scene-less and throttled (see `image_search_session::parked`), so an
//! agent working in a document that is not showing gets its pictures without
//! the user switching to it. Results are applied inside the lens, to the
//! document that owns the session; a document that is closed takes its
//! session, and with it every result still in flight, along.

use crate::chat_session;
use crate::DesktopApp;
use op_editor_core::DocumentId;
use op_host_native::WidgetHostNative;
use std::time::Instant;

/// Parked documents walked for image slots in one frame. A walk resolves a
/// whole page; polling finished jobs is not counted.
const PARKED_SCANS_PER_FRAME: usize = 1;

/// Hand the session the selected chat provider for its relevance judge until
/// one sticks (the env judge wins when configured; see
/// `ImageSearchSession::ensure_judge`). A provider configured mid-session is
/// still picked up until the first search actually spawns.
fn offer_judge(
    host: &WidgetHostNative,
    session: &mut crate::image_search_session::ImageSearchSession,
) {
    if session.judge_resolved() {
        return;
    }
    if let Some(provider) = chat_session::provider_for_selected_model(host) {
        let model = chat_session::selected_cli_model_id(host);
        session.ensure_judge(Some(std::sync::Arc::from(provider)), model);
    }
}

impl DesktopApp {
    /// One frame of image enrichment for every open document. Returns whether
    /// the screen needs a repaint.
    pub(crate) fn pump_image_enrichment(&mut self) -> bool {
        let on_screen = self.pump_on_screen_image_search();
        // `|` and not `||`: both documents' work runs every frame.
        on_screen | self.pump_parked_image_search(Instant::now())
    }

    fn pump_on_screen_image_search(&mut self) -> bool {
        offer_judge(&self.host, &mut self.runs.image_search);
        let (editor_state, layout_scene) = self.host.editor_state_mut_and_layout_scene();
        self.runs
            .image_search
            .enqueue_missing_with_scene(editor_state, layout_scene);
        if self
            .runs
            .image_search
            .poll_into_with_scene(editor_state, layout_scene)
        {
            self.host.mark_editor_state_dirty();
            return true;
        }
        false
    }

    /// Walk and poll the parked documents' sessions through the lens. Only a
    /// document with work (jobs in flight, or changed content past its scan
    /// gap) is entered at all.
    pub(crate) fn pump_parked_image_search(&mut self, now: Instant) -> bool {
        let mut changed = false;
        let mut scans_left = PARKED_SCANS_PER_FRAME;
        let mut in_flight =
            self.runs.image_search.pending_jobs() + self.documents.parked_image_jobs();
        for (index, work) in self.documents.parked_image_work(now) {
            let scan = work.scan
                && scans_left > 0
                && in_flight < crate::image_search_session::PARKED_PENDING_CAP;
            if !work.poll && !scan {
                continue;
            }
            let outcome = self.with_slot_runs(index, |host, runs| {
                let session = &mut runs.image_search;
                let before = session.pending_jobs();
                if scan {
                    session.note_parked_scan(now);
                    offer_judge(host, session);
                    session.enqueue_missing(host.editor_state());
                }
                let applied = session.poll_into(host.editor_state_mut());
                if applied {
                    host.mark_editor_state_dirty();
                }
                (applied, before, session.pending_jobs())
            });
            // A document the lens refused (a collaboration session is bound to
            // it) is skipped and costs no budget.
            let Some((applied, before, after)) = outcome else {
                continue;
            };
            changed |= applied;
            scans_left -= usize::from(scan);
            in_flight = (in_flight + after).saturating_sub(before);
        }
        changed
    }

    /// Whether any open document has image work for the event loop to wait on.
    pub(crate) fn any_image_search_pending(&self) -> bool {
        self.runs.image_search.is_pending() || self.documents.parked_image_work_outstanding()
    }

    /// Drop the image session state of the document `id` (its content was
    /// replaced wholesale, so node ids may name different nodes now).
    pub(crate) fn reset_image_search_of(&mut self, id: DocumentId) {
        if id == self.documents.active_id() {
            self.runs.image_search.reset();
        } else if let Some(slot) = self
            .documents
            .index_of(id)
            .and_then(|index| self.documents.slot_mut(index))
        {
            slot.local.runs.image_search.reset();
        }
    }
}

#[cfg(test)]
#[path = "image_enrichment_host_tests.rs"]
mod tests;
