//! `DesktopApp` side of crash recovery: when to snapshot an unsaved document,
//! and when its snapshot stops being needed.
//!
//! Every snapshot goes through [`DesktopApp::snapshot_dirty_documents`] (or its
//! per-document form for an agent run). It captures the document on the UI
//! thread, from the live state or, for a parked tab, through the host's lens
//! (`with_parked_document`, so the tab is never switched to), and hands the
//! capture to the writer thread. Three things trigger it:
//!
//! * the per-loop tick ([`DesktopApp::pump_recovery`]): a dirty document whose
//!   revision changed is snapshotted once 10 s have passed since the change was
//!   first seen, and not while a drag or a text edit is in progress;
//! * a launching agent run (`SnapshotReason::PreRun`), written before the run
//!   starts;
//! * process exit (`SnapshotReason::Exit`), written within a 2 s budget.
//!
//! An entry is removed when its document is saved, closed on purpose, or the
//! user confirmed a quit (see `recovery_quit`). Startup recovery is in
//! `recovery_startup`.

use crate::recovery_error::RecoveryError;
use crate::recovery_store::{RecoveryStore, SnapshotInfo, SnapshotReason, WriteOutcome};
use crate::recovery_worker::{RecoveryWriter, WriteJob};
use crate::DesktopApp;
use op_editor_core::DocumentId;
use op_host_services::doc_io::CanonicalSaveSnapshot;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a document may stay unsnapshotted after its first unsaved edit.
pub(crate) const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(10);
/// The per-loop check runs at most this often: the loop wakes for every
/// pointer move, and the check has nothing to do between them.
const TICK_EVERY: Duration = Duration::from_millis(250);
/// How soon to look again after a snapshot was held back by a drag or an edit.
const GESTURE_RETRY: Duration = Duration::from_secs(1);
/// The most the process waits for snapshot writes when it is exiting or an
/// agent run is about to start.
pub(crate) const SYNC_BUDGET: Duration = Duration::from_secs(2);

/// What the recovery code remembers about one open document.
#[derive(Default)]
struct SlotTrack {
    /// `(generation, revision)` of the last snapshot known to be on disk.
    snapshotted: Option<(u64, u64)>,
    /// Whether an entry for this document exists (or may by now).
    has_entry: bool,
    /// The revision of a write still running on the worker.
    in_flight: Option<(u64, u64)>,
    /// When the next periodic snapshot is due, once an unsaved edit was seen.
    due: Option<Instant>,
}

/// The app-level recovery state. Disabled (no store) when there is no config
/// directory, and in unit tests unless a test installs a store.
pub(crate) struct RecoveryHost {
    store: Option<Arc<RecoveryStore>>,
    writer: Option<RecoveryWriter>,
    tracks: HashMap<DocumentId, SlotTrack>,
    interval: Duration,
    /// Set by a quit the user confirmed: every document was saved or discarded
    /// on purpose, so the exit writes nothing and removes this run's entries.
    pub(crate) quit_confirmed: bool,
    /// The documents the user chose Don't Save for in that quit.
    pub(crate) discarded: std::collections::HashSet<DocumentId>,
    pub(crate) exit_done: bool,
    last_tick: Option<Instant>,
}

impl RecoveryHost {
    pub fn disabled() -> Self {
        Self {
            store: None,
            writer: None,
            tracks: HashMap::new(),
            interval: SNAPSHOT_INTERVAL,
            quit_confirmed: false,
            discarded: std::collections::HashSet::new(),
            exit_done: false,
            last_tick: None,
        }
    }

    /// The store beside the user's settings file.
    pub fn for_user() -> Self {
        let Some(dir) = op_host_services::settings_io::settings_dir() else {
            eprintln!("[recovery] no settings directory: crash recovery is off");
            return Self::disabled();
        };
        let pid = std::process::id();
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        Self::with_store(RecoveryStore::new(
            dir.join("recovery"),
            format!("{millis:x}p{pid}"),
            pid,
        ))
    }

    pub fn with_store(store: RecoveryStore) -> Self {
        let store = Arc::new(store);
        Self {
            writer: Some(RecoveryWriter::new(Arc::clone(&store))),
            store: Some(store),
            ..Self::disabled()
        }
    }

    pub fn store(&self) -> Option<&Arc<RecoveryStore>> {
        self.store.as_ref()
    }

    /// Change the periodic interval (tests use zero).
    #[cfg(test)]
    pub fn set_interval(&mut self, interval: Duration) {
        self.interval = interval;
    }

    /// The soonest moment a periodic snapshot is due, so an idle app wakes for
    /// it. Documents with a write in flight are left out: their result is
    /// collected on the next loop turn.
    pub fn next_deadline(&self) -> Option<Instant> {
        let due = self
            .tracks
            .values()
            .filter(|track| track.in_flight.is_none())
            .filter_map(|track| track.due)
            .min()?;
        // Never earlier than the next tick can act on it, or the loop would
        // spin until the throttle opens.
        Some(match self.last_tick {
            Some(last) => due.max(last + TICK_EVERY),
            None => due,
        })
    }

    /// Whether the throttled per-loop check should run now (and note that it
    /// did).
    fn tick_is_due(&mut self) -> bool {
        if self.store.is_none() {
            return false;
        }
        let now = Instant::now();
        if self
            .last_tick
            .is_some_and(|last| now.duration_since(last) < TICK_EVERY)
        {
            return false;
        }
        self.last_tick = Some(now);
        true
    }

    /// Wait for the writer to finish what it was given, up to `budget`.
    pub(crate) fn wait_idle(&self, budget: Duration) -> bool {
        self.writer
            .as_ref()
            .is_none_or(|writer| writer.wait_idle(budget))
    }

    pub(crate) fn has_entry(&self, id: DocumentId) -> bool {
        self.tracks.get(&id).is_some_and(|track| track.has_entry)
    }

    /// Fold the writer's finished jobs into the per-document bookkeeping.
    fn collect_results(&mut self) {
        let (Some(writer), Some(store)) = (self.writer.as_ref(), self.store.as_ref()) else {
            return;
        };
        let now = Instant::now();
        for result in writer.drain_results() {
            let Some(track) = self.tracks.get_mut(&DocumentId(result.document)) else {
                continue;
            };
            track.in_flight = None;
            match result.outcome {
                // A clear that ran after the write committed deleted the entry:
                // the result describes a file that is gone.
                Ok(WriteOutcome::Committed) if store.is_current(&result.ticket) => {
                    track.snapshotted = Some(result.revision);
                    track.has_entry = true;
                    track.due = None;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("[recovery] {error}; will retry");
                    track.due = Some(now + self.interval);
                }
            }
        }
    }
}

/// What the recovery code needs to know about one open document.
struct SlotView {
    id: DocumentId,
    dirty: bool,
    /// `(generation, revision)`.
    revision: (u64, u64),
    path: Option<PathBuf>,
    /// Untouched since it was created (nothing worth keeping).
    pristine: bool,
}

impl DesktopApp {
    fn slot_views(&mut self) -> Vec<SlotView> {
        self.sync_active_slot();
        let active = self.documents.active_index();
        let live = {
            let state = self.host.editor_state();
            (
                state.is_dirty(),
                (state.document_generation(), state.document_revision()),
                state.document_revision() == 0
                    && !state.is_dirty()
                    && !state.history.can_undo()
                    && state.chat.is_untouched(),
            )
        };
        (0..self.documents.len())
            .filter_map(|index| {
                let slot = self.documents.slot_mut(index)?;
                let path = slot.local.path.clone();
                let id = slot.id;
                let (dirty, revision, pristine) = if index == active {
                    live
                } else {
                    let scope = slot.parked.as_ref()?.scope();
                    (
                        scope.is_dirty(),
                        (scope.document_generation(), scope.document_revision()),
                        scope.is_pristine(),
                    )
                };
                Some(SlotView {
                    id,
                    dirty,
                    revision,
                    path,
                    pristine,
                })
            })
            .collect()
    }

    /// Capture the document at `index` for the writer. A parked document is
    /// borrowed through the lens without being switched to; a collaboration
    /// session that forbids the lens makes this fail (and the caller retry).
    fn capture_recovery_snapshot(
        &mut self,
        index: usize,
    ) -> Result<Box<CanonicalSaveSnapshot>, RecoveryError> {
        if index == self.documents.active_index() {
            return Ok(Box::new(CanonicalSaveSnapshot::capture(
                self.host.editor_state(),
            )));
        }
        let Some(mut parked) = self.documents.take_parked(index) else {
            return Err(RecoveryError::Capture {
                detail: "the document is not parked".to_string(),
            });
        };
        let captured = self.host.with_parked_document(&mut parked, |host| {
            CanonicalSaveSnapshot::capture(host.editor_state())
        });
        self.documents.restore_parked(index, parked);
        captured.map(Box::new).ok_or(RecoveryError::Capture {
            detail: "a collaboration session does not lend the parked document".to_string(),
        })
    }

    fn recovery_label(&self, path: Option<&PathBuf>) -> String {
        match path.and_then(|path| path.file_name()) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => {
                let locale = self.host.editor_state().editor_ui.locale;
                op_i18n::translate(locale, "common.untitled").to_string()
            }
        }
    }

    /// Capture one document and queue its write. `false` when nothing was
    /// queued (the failure is logged and the caller retries later).
    fn submit_recovery_snapshot(&mut self, view: &SlotView, reason: SnapshotReason) -> bool {
        let Some(store) = self.recovery.store().cloned() else {
            return false;
        };
        let Some(index) = self.documents.index_of(view.id) else {
            return false;
        };
        let snapshot = match self.capture_recovery_snapshot(index) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("[recovery] {error}; will retry");
                self.recovery.tracks.entry(view.id).or_default().due =
                    Some(Instant::now() + self.recovery.interval);
                return false;
            }
        };
        let job = WriteJob {
            document: view.id.0,
            revision: view.revision,
            ticket: store.ticket(view.id.0),
            snapshot,
            info: SnapshotInfo {
                original_path: view.path.clone(),
                label: self.recovery_label(view.path.as_ref()),
                reason,
            },
        };
        let queued = match self.recovery.writer.as_mut() {
            Some(writer) => writer.submit(job),
            None => return false,
        };
        let track = self.recovery.tracks.entry(view.id).or_default();
        match queued {
            Ok(()) => {
                track.in_flight = Some(view.revision);
                track.due = None;
                true
            }
            Err(error) => {
                eprintln!("[recovery] {error}; will retry");
                track.due = Some(Instant::now() + self.recovery.interval);
                false
            }
        }
    }

    /// Snapshot every unsaved document, the one place all triggers go through.
    ///
    /// `Autosnapshot` is the periodic rule (see the module docs) and returns
    /// at once; the write runs on the worker. `PreRun` and `Exit` snapshot
    /// every unsaved document now and wait for the writes, up to
    /// [`SYNC_BUDGET`]. Returns how many snapshots were queued.
    pub(crate) fn snapshot_dirty_documents(&mut self, reason: SnapshotReason) -> usize {
        self.snapshot_documents(reason, None, false)
    }

    /// Snapshot one unsaved document at once, whatever the interval, and wait
    /// for the write (a freshly recovered tab takes over its old entry only
    /// once it has its own).
    pub(crate) fn snapshot_document_now(
        &mut self,
        id: DocumentId,
        reason: SnapshotReason,
    ) -> usize {
        self.snapshot_documents(reason, Some(id), true)
    }

    /// [`Self::snapshot_dirty_documents`] for the one document an agent run is
    /// about to start in (the document on screen). A document that is not
    /// dirty is still written when it is untitled and has content: it has no
    /// file to fall back on.
    pub(crate) fn snapshot_before_run(&mut self) -> usize {
        let id = self.documents.active_id();
        self.snapshot_documents(SnapshotReason::PreRun, Some(id), true)
    }

    fn snapshot_documents(
        &mut self,
        reason: SnapshotReason,
        only: Option<DocumentId>,
        immediate: bool,
    ) -> usize {
        // Only the periodic rule waits for its interval and writes in the
        // background; every other trigger writes now and waits for the disk.
        let immediate = immediate || reason != SnapshotReason::Autosnapshot;
        if self.recovery.store().is_none() {
            return 0;
        }
        self.recovery.collect_results();
        let now = Instant::now();
        let interval = self.recovery.interval;
        let drag_or_edit = self.host.pointer_gesture_in_progress();
        let active = self.documents.active_id();
        let mut queued = 0;
        for view in self.slot_views() {
            if only.is_some_and(|id| id != view.id) {
                continue;
            }
            let wanted = view.dirty
                || (reason == SnapshotReason::PreRun && view.path.is_none() && !view.pristine);
            let track = self.recovery.tracks.entry(view.id).or_default();
            if !wanted {
                track.due = None;
                continue;
            }
            let current = track.has_entry && track.snapshotted == Some(view.revision);
            if current {
                track.due = None;
                continue;
            }
            if !immediate {
                if track.in_flight.is_some() {
                    continue;
                }
                let due = *track.due.get_or_insert(now + interval);
                if now < due {
                    continue;
                }
                if drag_or_edit && view.id == active {
                    track.due = Some(now + GESTURE_RETRY);
                    continue;
                }
            }
            if self.submit_recovery_snapshot(&view, reason) {
                queued += 1;
            }
        }
        if immediate && queued > 0 {
            if !self.recovery.wait_idle(SYNC_BUDGET) {
                eprintln!("[recovery] snapshot writes still running after {SYNC_BUDGET:?}");
            }
            self.recovery.collect_results();
        }
        queued
    }

    /// [`Self::pump_recovery`] for the event loop's wake-ups, at most four
    /// times a second.
    pub(crate) fn pump_recovery_on_wake(&mut self) {
        if self.recovery.tick_is_due() {
            self.pump_recovery();
        }
    }

    /// The per-loop check: collect finished writes, drop entries that are no
    /// longer needed, and snapshot what is due. Cheap when nothing is unsaved.
    pub(crate) fn pump_recovery(&mut self) {
        if self.recovery.store().is_none() {
            return;
        }
        self.clear_obsolete_recovery_entries();
        self.snapshot_dirty_documents(SnapshotReason::Autosnapshot);
    }

    /// Remove the entry of a document that was saved (it is clean and bound to
    /// a file, which now holds its content) or whose tab is gone. An untitled
    /// document keeps the entry a run's pre-snapshot made: it has no file.
    pub(crate) fn clear_obsolete_recovery_entries(&mut self) {
        if self.recovery.store().is_none() {
            return;
        }
        self.recovery.collect_results();
        let views = self.slot_views();
        let tracked: Vec<DocumentId> = self.recovery.tracks.keys().copied().collect();
        for id in tracked {
            match views.iter().find(|view| view.id == id) {
                None => self.recovery_forget(id),
                Some(view) if !view.dirty && view.path.is_some() => self.recovery_clear(id),
                Some(_) => {}
            }
        }
    }

    /// Delete the entry of `id` and start over for it: its next unsaved edit
    /// is snapshotted afresh.
    pub(crate) fn recovery_clear(&mut self, id: DocumentId) {
        let Some(store) = self.recovery.store().cloned() else {
            return;
        };
        if let Some(track) = self.recovery.tracks.get_mut(&id) {
            if !track.has_entry && track.in_flight.is_none() {
                track.due = None;
                return;
            }
            track.snapshotted = None;
            track.has_entry = false;
            track.due = None;
        } else {
            return;
        }
        if let Err(error) = store.clear(id.0) {
            eprintln!("[recovery] {error}");
        }
    }

    /// The document is gone (its tab closed on purpose): delete its entry and
    /// forget it.
    pub(crate) fn recovery_forget(&mut self, id: DocumentId) {
        let Some(store) = self.recovery.store().cloned() else {
            return;
        };
        if let Err(error) = store.clear(id.0) {
            eprintln!("[recovery] {error}");
        }
        self.recovery.tracks.remove(&id);
    }

    /// The exit after a quit the user confirmed: every document was saved or
    /// discarded on purpose, so this run's entries go. The exception is a
    /// document that is unsaved again and was not thrown away: stopping an
    /// agent finalizes its design after the prompt, so that edit exists
    /// nowhere else and is snapshotted (and only it keeps an entry).
    pub(crate) fn recovery_exit_after_confirmed_quit(&mut self) {
        let Some(store) = self.recovery.store().cloned() else {
            return;
        };
        self.recovery.wait_idle(SYNC_BUDGET);
        let keep: Vec<DocumentId> = self
            .slot_views()
            .into_iter()
            .filter(|view| view.dirty && !self.recovery.discarded.contains(&view.id))
            .map(|view| view.id)
            .collect();
        if keep.is_empty() {
            store.clear_launch();
            return;
        }
        let tracked: Vec<DocumentId> = self.recovery.tracks.keys().copied().collect();
        for id in tracked.into_iter().filter(|id| !keep.contains(id)) {
            self.recovery_forget(id);
        }
        for id in keep {
            self.snapshot_document_now(id, SnapshotReason::Exit);
        }
    }

    /// Wait for in-flight snapshot writes and collect their results; tests use
    /// it to look at the disk deterministically.
    #[cfg(test)]
    pub(crate) fn flush_recovery(&mut self) {
        assert!(self.recovery.wait_idle(Duration::from_secs(30)));
        self.recovery.collect_results();
    }
}

#[cfg(test)]
#[path = "recovery_test_support.rs"]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "recovery_host_tests.rs"]
mod tests;
