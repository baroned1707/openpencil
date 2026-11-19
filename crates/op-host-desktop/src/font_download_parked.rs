//! Missing-font downloads for the documents that are NOT on screen.
//!
//! The on-screen document's missing families come from the prompt the editor
//! keeps for it (`FontDownloadHost::drain`). A parked document has no prompt,
//! so while "Download missing fonts automatically" is on, each parked document
//! that changed is checked through the host's lens, at most one per frame and
//! at most once per [`PARKED_FONT_SCAN_GAP`], and what it lacks is requested
//! from the SAME fetcher: one family downloads once however many documents
//! use it, and the concurrency, per-file and per-session limits are shared.
//! Faces wanted by several documents are merged into one request: what a scan
//! finds waits until no parked document is left to check, then goes out once.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use op_editor_core::missing_font_faces::document_font_faces;
use op_editor_core::missing_fonts::detect_missing_fonts;
use op_editor_core::{DocumentId, EditorState};
use op_font_fetch::{FaceRequest, FamilyStatus};

use super::FontDownloadHost;
use crate::DesktopApp;

/// A parked document is checked at most this often. Detection scans every
/// font in the document, and an agent keeps changing the revision.
const PARKED_FONT_SCAN_GAP: Duration = Duration::from_millis(500);

/// Parked documents checked in one frame.
const PARKED_FONT_SCANS_PER_FRAME: usize = 1;

/// A family and the faces wanted of it.
type Wanted = (String, BTreeSet<FaceRequest>);

/// Which revision of each parked document was last checked and when, and the
/// families those checks found that have not been requested yet.
#[derive(Default)]
pub(super) struct ParkedFontScans {
    seen: HashMap<DocumentId, (u64, Instant)>,
    /// Keyed by the lower-cased family so documents spelling it differently
    /// share one request.
    waiting: BTreeMap<String, Wanted>,
}

impl ParkedFontScans {
    /// Whether the document at `revision` should be checked at `now`: it
    /// changed since the last check and the gap has passed.
    fn due(&self, id: DocumentId, revision: u64, now: Instant) -> bool {
        self.seen.get(&id).is_none_or(|&(seen, at)| {
            seen != revision && now.saturating_duration_since(at) >= PARKED_FONT_SCAN_GAP
        })
    }

    fn record(&mut self, id: DocumentId, revision: u64, now: Instant) {
        self.seen.insert(id, (revision, now));
    }

    /// Forget documents that are no longer open.
    fn retain_open(&mut self, open: &[DocumentId]) {
        self.seen.retain(|id, _| open.contains(id));
    }

    /// Hold what a check found until the batch is sent.
    fn queue(&mut self, found: Vec<Wanted>) {
        for (family, faces) in found {
            self.waiting
                .entry(family.to_ascii_lowercase())
                .or_insert_with(|| (family, BTreeSet::new()))
                .1
                .extend(faces);
        }
    }

    /// The batch, once nothing more is expected for it.
    fn take_batch(&mut self) -> Vec<Wanted> {
        std::mem::take(&mut self.waiting).into_values().collect()
    }
}

/// Every family `state` uses that the system does not provide, with the faces
/// the document needs of each.
fn missing_families(state: &EditorState) -> Vec<Wanted> {
    detect_missing_fonts(state)
        .map(|prompt| {
            prompt
                .entries
                .into_iter()
                .filter(|entry| !entry.resolved)
                .map(|entry| {
                    let faces = document_font_faces(state, &entry.family)
                        .into_iter()
                        .map(|(weight, italic)| FaceRequest { weight, italic })
                        .collect();
                    (entry.family, faces)
                })
                .collect()
        })
        .unwrap_or_default()
}

impl FontDownloadHost {
    /// Request the families nobody has tried yet (the on-screen document's
    /// rule: a failure is never retried automatically). Returns whether a
    /// download was queued.
    fn request_untried(&mut self, batch: Vec<Wanted>) -> bool {
        let mut queued = false;
        for (family, faces) in batch {
            let untried = matches!(
                self.fetcher.status(&family),
                None | Some(FamilyStatus::Ready)
            );
            if untried {
                queued |= self.fetcher.request_download(&family, faces, false);
            }
        }
        queued
    }
}

impl DesktopApp {
    /// One frame of the font downloader: the on-screen document's prompt and
    /// finished downloads, then the parked documents. Returns whether the
    /// screen needs a repaint.
    pub(crate) fn pump_font_downloads(&mut self) -> bool {
        let on_screen = self.font_downloads.drain(&mut self.host);
        on_screen | self.pump_parked_font_downloads(Instant::now())
    }

    /// Check the parked documents that are due for missing fonts and start
    /// their downloads. Starting a download changes a status the screen shows.
    pub(crate) fn pump_parked_font_downloads(&mut self, now: Instant) -> bool {
        let (auto, fonts_loaded) = {
            let ui = &self.host.editor_state().editor_ui;
            (
                ui.agent_settings.auto_download_missing_fonts,
                ui.system_fonts_loaded,
            )
        };
        if !auto || !fonts_loaded {
            return false;
        }
        self.font_downloads
            .parked_scans
            .retain_open(&self.documents.open_ids());
        let mut scans_left = PARKED_FONT_SCANS_PER_FRAME;
        let candidates = self.documents.parked_font_candidates();
        for &(index, id, revision) in &candidates {
            if scans_left == 0 {
                break;
            }
            if !self.font_downloads.parked_scans.due(id, revision, now) {
                continue;
            }
            // A document the lens refuses is simply left for a later frame.
            let Some(missing) =
                self.with_slot_runs(index, |host, _| missing_families(host.editor_state()))
            else {
                continue;
            };
            self.font_downloads.parked_scans.record(id, revision, now);
            scans_left -= 1;
            self.font_downloads.parked_scans.queue(missing);
        }
        // Send the batch only when every changed document has been checked, so
        // the faces they want of one family go out together.
        let more_to_check = candidates
            .iter()
            .any(|&(_, id, revision)| self.font_downloads.parked_scans.due(id, revision, now));
        if more_to_check {
            return false;
        }
        let batch = self.font_downloads.parked_scans.take_batch();
        self.font_downloads.request_untried(batch)
    }
}

#[cfg(test)]
#[path = "font_download_parked_tests.rs"]
mod tests;
