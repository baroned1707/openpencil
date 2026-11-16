//! Crash-recovery snapshots of unsaved documents, on disk.
//!
//! The store is a `recovery/` folder beside the settings file. It holds at most
//! one live entry per document of a launch, and an entry is two files:
//!
//! * `<launch>-<docid>.op` — the canonical document, with `editorMeta` and
//!   thumbnails, written by the same writer a Save uses;
//! * `<launch>-<docid>.json` — a [`RecoveryManifest`] saying whose snapshot it
//!   is (original path, label, time, process, why it was taken).
//!
//! The manifest is the commit point: an `.op` without a manifest is an orphan
//! and is never offered. Both files are written to a temporary name and renamed
//! into place, so a crash or a full disk leaves the previous entry untouched.
//!
//! Writers run on a worker thread and a delete runs on the UI thread, so a
//! write that finishes after its document was cleared (or after a newer
//! snapshot of the same document) must not resurrect or regress the entry. A
//! [`Ticket`] taken when the snapshot is *captured* carries the document's
//! clear epoch and a sequence number; the commit step, under one lock, refuses a
//! write whose epoch was bumped by a clear or whose sequence is older than the
//! one already committed.
//!
//! The store is bounded: entries older than 30 days are dropped, and the total
//! never exceeds 200 MB (oldest entries go first).

use crate::recovery_error::RecoveryError;
use op_host_services::doc_io::{self, atomic_file, CanonicalSaveSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Entries older than this are deleted by [`RecoveryStore::prune`].
pub(crate) const MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;
/// The store never keeps more than this many bytes.
pub(crate) const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
/// A file without a manifest (or a stray temporary) is only deleted once it is
/// this old, so a write of another running instance is never cut short.
const ORPHAN_GRACE_SECS: u64 = 10 * 60;

/// Why a snapshot was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum SnapshotReason {
    /// The periodic check found unsaved edits.
    #[serde(rename = "autosnapshot")]
    Autosnapshot,
    /// An agent run was about to start.
    #[serde(rename = "pre-run")]
    PreRun,
    /// The process is exiting.
    #[serde(rename = "exit")]
    Exit,
}

/// The sidecar of a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecoveryManifest {
    /// The file the document was bound to; `None` for an untitled document.
    pub original_path: Option<String>,
    /// What the user called it (its file name, or "Untitled").
    pub label: String,
    pub saved_at_unix: u64,
    /// The process that wrote it, so another live instance's entries are left
    /// alone.
    pub app_pid: u32,
    pub launch_id: String,
    pub reason: SnapshotReason,
    /// Whether the document had a file on disk when it was snapshotted, i.e.
    /// whether recovering it can write back over something.
    pub base_saved: bool,
    /// The document's id within its launch (`d<N>`).
    pub document_id: u64,
}

/// What the caller says about the document being snapshotted.
#[derive(Debug, Clone)]
pub(crate) struct SnapshotInfo {
    pub original_path: Option<PathBuf>,
    pub label: String,
    pub reason: SnapshotReason,
}

/// Permission to commit one captured snapshot (see the module docs).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ticket {
    document: u64,
    sequence: u64,
    clear_epoch: u64,
}

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteOutcome {
    Committed,
    /// A clear or a newer snapshot got there first; nothing was changed.
    Superseded,
}

/// One entry found on disk.
#[derive(Debug, Clone)]
pub(crate) struct RecoveryEntry {
    pub manifest: RecoveryManifest,
    pub document_path: PathBuf,
    pub manifest_path: PathBuf,
    /// Size of both files.
    pub bytes: u64,
}

/// What a scan found: the usable entries and how many manifests were skipped
/// as corrupt, foreign or missing their document.
#[derive(Debug, Default)]
pub(crate) struct ScanReport {
    pub entries: Vec<RecoveryEntry>,
    pub skipped: usize,
}

#[derive(Default)]
struct Ledger {
    /// Highest sequence committed per document.
    committed: HashMap<u64, u64>,
    /// Number of times each document was cleared.
    clears: HashMap<u64, u64>,
}

pub(crate) struct RecoveryStore {
    dir: PathBuf,
    launch_id: String,
    pid: u32,
    max_age_secs: u64,
    max_total_bytes: u64,
    next_sequence: AtomicU64,
    ledger: Mutex<Ledger>,
}

/// The kind of file a name in the store directory is, by our naming scheme.
#[derive(Debug, PartialEq, Eq)]
enum FileKind {
    Document,
    Manifest,
    /// `<launch>-<docid>.op.pending`: a snapshot waiting for its commit.
    Pending,
    /// The writer's own hidden temporary (`.<name>.<pid>.<n>.tmp`).
    Temporary,
    Foreign,
}

/// `<launch>-<docid>` of a store file stem: the launch id and the document id.
fn parse_stem(stem: &str) -> Option<(&str, u64)> {
    let (launch, document) = stem.rsplit_once('-')?;
    let document = document.parse().ok()?;
    (!launch.is_empty()).then_some((launch, document))
}

fn classify(name: &str) -> FileKind {
    if name.starts_with('.') && name.ends_with(".tmp") {
        return FileKind::Temporary;
    }
    let owned = |stem: &str| parse_stem(stem).is_some();
    if let Some(stem) = name.strip_suffix(".op.pending") {
        return if owned(stem) {
            FileKind::Pending
        } else {
            FileKind::Foreign
        };
    }
    if let Some(stem) = name.strip_suffix(".op") {
        return if owned(stem) {
            FileKind::Document
        } else {
            FileKind::Foreign
        };
    }
    if let Some(stem) = name.strip_suffix(".json") {
        return if owned(stem) {
            FileKind::Manifest
        } else {
            FileKind::Foreign
        };
    }
    FileKind::Foreign
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn modified_unix(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs())
}

impl RecoveryStore {
    /// A store over `dir` for the launch `launch_id` of process `pid`. The
    /// directory is created on first write. `launch_id` must not contain `-`.
    pub fn new(dir: PathBuf, launch_id: String, pid: u32) -> Self {
        debug_assert!(!launch_id.contains('-'), "launch ids are dash-free");
        Self {
            dir,
            launch_id,
            pid,
            max_age_secs: MAX_AGE_SECS,
            max_total_bytes: MAX_TOTAL_BYTES,
            next_sequence: AtomicU64::new(1),
            ledger: Mutex::new(Ledger::default()),
        }
    }

    /// The same store with other bounds (tests use tiny ones).
    #[cfg(test)]
    pub fn with_limits(mut self, max_age_secs: u64, max_total_bytes: u64) -> Self {
        self.max_age_secs = max_age_secs;
        self.max_total_bytes = max_total_bytes;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn stem(&self, document: u64) -> String {
        format!("{}-{document}", self.launch_id)
    }

    fn document_path(&self, document: u64) -> PathBuf {
        self.dir.join(format!("{}.op", self.stem(document)))
    }

    fn manifest_path(&self, document: u64) -> PathBuf {
        self.dir.join(format!("{}.json", self.stem(document)))
    }

    fn pending_path(&self, document: u64) -> PathBuf {
        self.dir.join(format!("{}.op.pending", self.stem(document)))
    }

    fn lock_ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.ledger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Permission for a snapshot captured now. Take it when the snapshot is
    /// taken, not when its write starts.
    pub fn ticket(&self, document: u64) -> Ticket {
        let clear_epoch = self
            .lock_ledger()
            .clears
            .get(&document)
            .copied()
            .unwrap_or(0);
        Ticket {
            document,
            sequence: self.next_sequence.fetch_add(1, Ordering::Relaxed),
            clear_epoch,
        }
    }

    /// Write `snapshot` as the one entry of `ticket`'s document, replacing the
    /// previous one atomically. Safe to call from any thread.
    pub fn write(
        &self,
        ticket: Ticket,
        snapshot: &CanonicalSaveSnapshot,
        info: &SnapshotInfo,
    ) -> Result<WriteOutcome, RecoveryError> {
        std::fs::create_dir_all(&self.dir).map_err(|error| RecoveryError::Directory {
            path: self.dir.display().to_string(),
            detail: error.to_string(),
        })?;
        let pending = self.pending_path(ticket.document);
        // The slow part (serialization and file IO) runs outside the lock.
        doc_io::save_snapshot_to_path(snapshot, &pending).map_err(|error| {
            RecoveryError::Document {
                path: pending.display().to_string(),
                detail: error.to_string(),
            }
        })?;
        let manifest = RecoveryManifest {
            original_path: info
                .original_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            label: info.label.clone(),
            saved_at_unix: unix_now(),
            app_pid: self.pid,
            launch_id: self.launch_id.clone(),
            reason: info.reason,
            base_saved: info.original_path.is_some(),
            document_id: ticket.document,
        };
        let outcome = {
            let mut ledger = self.lock_ledger();
            let current_epoch = ledger.clears.get(&ticket.document).copied().unwrap_or(0);
            let committed = ledger.committed.get(&ticket.document).copied().unwrap_or(0);
            if current_epoch != ticket.clear_epoch || committed > ticket.sequence {
                let _ = std::fs::remove_file(&pending);
                WriteOutcome::Superseded
            } else {
                self.commit(ticket.document, &pending, &manifest)?;
                ledger.committed.insert(ticket.document, ticket.sequence);
                WriteOutcome::Committed
            }
        };
        if outcome == WriteOutcome::Committed {
            self.prune(unix_now());
        }
        Ok(outcome)
    }

    /// Move the pending snapshot into place, then publish its manifest.
    fn commit(
        &self,
        document: u64,
        pending: &Path,
        manifest: &RecoveryManifest,
    ) -> Result<(), RecoveryError> {
        let document_path = self.document_path(document);
        atomic_file::replace_file(pending, &document_path).map_err(|error| {
            RecoveryError::Document {
                path: document_path.display().to_string(),
                detail: error.to_string(),
            }
        })?;
        let manifest_path = self.manifest_path(document);
        let fail = |detail: String| RecoveryError::Manifest {
            path: manifest_path.display().to_string(),
            detail,
        };
        let bytes = serde_json::to_vec_pretty(manifest).map_err(|e| fail(e.to_string()))?;
        let (temporary, mut file) =
            atomic_file::create_sibling_temp(&manifest_path).map_err(|e| fail(e.to_string()))?;
        let written = std::io::Write::write_all(&mut file, &bytes)
            .and_then(|()| file.sync_all())
            .map_err(|e| fail(e.to_string()));
        drop(file);
        let published = written.and_then(|()| {
            atomic_file::replace_file(&temporary, &manifest_path).map_err(|e| fail(e.to_string()))
        });
        if published.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        published
    }

    /// Whether no clear has happened since `ticket` was taken, i.e. a write
    /// that committed under it still describes a live entry.
    pub fn is_current(&self, ticket: &Ticket) -> bool {
        let clears = self.lock_ledger().clears.get(&ticket.document).copied();
        clears.unwrap_or(0) == ticket.clear_epoch
    }

    /// Delete every entry of this launch: after a quit the user confirmed,
    /// each document was saved or discarded on purpose, so nothing of this run
    /// is worth recovering. Callers wait for the writer to go idle first.
    pub fn clear_launch(&self) {
        let _ledger = self.lock_ledger();
        let Ok(listing) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let prefix = format!("{}-", self.launch_id);
        for item in listing.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            let trimmed = name.trim_start_matches('.');
            if trimmed.starts_with(&prefix) && classify(&name) != FileKind::Foreign {
                remove_logged(&item.path());
            }
        }
    }

    /// Delete the document's entry, whatever its reason, and make any write
    /// still in flight for it commit nothing. A missing entry is not an error.
    pub fn clear(&self, document: u64) -> Result<(), RecoveryError> {
        let mut ledger = self.lock_ledger();
        *ledger.clears.entry(document).or_insert(0) += 1;
        ledger.committed.remove(&document);
        let mut first_error = None;
        for path in [
            self.manifest_path(document),
            self.document_path(document),
            self.pending_path(document),
        ] {
            if let Err(error) = std::fs::remove_file(&path) {
                if error.kind() != std::io::ErrorKind::NotFound && first_error.is_none() {
                    first_error = Some(RecoveryError::Remove {
                        path: path.display().to_string(),
                        detail: error.to_string(),
                    });
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Delete an entry found by [`Self::scan`] (a previous launch's, or one the
    /// user discarded).
    pub fn remove_entry(&self, entry: &RecoveryEntry) -> Result<(), RecoveryError> {
        let _ledger = self.lock_ledger();
        let mut first_error = None;
        for path in [&entry.manifest_path, &entry.document_path] {
            if let Err(error) = std::fs::remove_file(path) {
                if error.kind() != std::io::ErrorKind::NotFound && first_error.is_none() {
                    first_error = Some(RecoveryError::Remove {
                        path: path.display().to_string(),
                        detail: error.to_string(),
                    });
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Read the directory. Entries of this launch and entries whose process
    /// `is_alive` reports as a live process other than this one are left out;
    /// manifests that are corrupt, do not match their file name or have lost
    /// their document are counted as skipped and left in place.
    pub fn scan(&self, is_alive: impl Fn(u32) -> bool) -> ScanReport {
        let mut report = ScanReport::default();
        let Ok(listing) = std::fs::read_dir(&self.dir) else {
            return report;
        };
        for item in listing.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if classify(&name) != FileKind::Manifest {
                continue;
            }
            match self.read_entry(&item.path(), &name) {
                Some(entry) => {
                    let ours = entry.manifest.launch_id == self.launch_id;
                    let other_instance =
                        entry.manifest.app_pid != self.pid && is_alive(entry.manifest.app_pid);
                    if !ours && !other_instance {
                        report.entries.push(entry);
                    }
                }
                None => report.skipped += 1,
            }
        }
        report.entries.sort_by(|a, b| {
            (a.manifest.saved_at_unix, &a.manifest_path)
                .cmp(&(b.manifest.saved_at_unix, &b.manifest_path))
        });
        report
    }

    /// A valid entry for the manifest at `path`, or `None` when it is corrupt,
    /// foreign (its content disagrees with its name) or has no document.
    fn read_entry(&self, path: &Path, name: &str) -> Option<RecoveryEntry> {
        let stem = name.strip_suffix(".json")?;
        let (launch, document) = parse_stem(stem)?;
        let manifest_bytes = std::fs::read(path).ok()?;
        let manifest: RecoveryManifest = serde_json::from_slice(&manifest_bytes).ok()?;
        if manifest.launch_id != launch || manifest.document_id != document {
            return None;
        }
        let document_path = self.dir.join(format!("{stem}.op"));
        let document_len = std::fs::metadata(&document_path).ok()?.len();
        (document_len > 0).then(|| RecoveryEntry {
            manifest,
            document_path,
            manifest_path: path.to_path_buf(),
            bytes: document_len + manifest_bytes.len() as u64,
        })
    }

    /// Enforce the bounds: drop entries older than the age limit, remove stale
    /// orphans, then drop the oldest entries until the total fits. Never fails;
    /// what cannot be removed is logged.
    pub fn prune(&self, now_unix: u64) {
        let _ledger = self.lock_ledger();
        let Ok(listing) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut entries: Vec<RecoveryEntry> = Vec::new();
        let mut loose: Vec<PathBuf> = Vec::new();
        let mut manifest_stems = std::collections::HashSet::new();
        for item in listing.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            match classify(&name) {
                FileKind::Manifest => match self.read_entry(&item.path(), &name) {
                    Some(entry) => {
                        if let Some(stem) = name.strip_suffix(".json") {
                            manifest_stems.insert(stem.to_owned());
                        }
                        entries.push(entry);
                    }
                    None => loose.push(item.path()),
                },
                FileKind::Pending | FileKind::Temporary => loose.push(item.path()),
                FileKind::Document => {
                    let stem = name.strip_suffix(".op").unwrap_or(&name).to_owned();
                    if !self.dir.join(format!("{stem}.json")).exists() {
                        loose.push(item.path());
                    }
                }
                FileKind::Foreign => {}
            }
        }
        // Orphans and corrupt manifests, once old enough not to be a write in
        // progress.
        for path in loose {
            let old_enough = modified_unix(&path)
                .is_some_and(|modified| now_unix.saturating_sub(modified) >= ORPHAN_GRACE_SECS);
            if old_enough {
                remove_logged(&path);
            }
        }
        let (expired, mut kept): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| {
            now_unix.saturating_sub(entry.manifest.saved_at_unix) > self.max_age_secs
        });
        for entry in &expired {
            remove_entry_files(entry);
        }
        kept.sort_by(|a, b| {
            (a.manifest.saved_at_unix, &a.manifest_path)
                .cmp(&(b.manifest.saved_at_unix, &b.manifest_path))
        });
        let mut total: u64 = kept.iter().map(|entry| entry.bytes).sum();
        for entry in &kept {
            if total <= self.max_total_bytes {
                break;
            }
            eprintln!(
                "[recovery] over {} bytes: dropping the oldest entry {}",
                self.max_total_bytes,
                entry.manifest_path.display()
            );
            remove_entry_files(entry);
            total = total.saturating_sub(entry.bytes);
        }
    }
}

fn remove_entry_files(entry: &RecoveryEntry) {
    remove_logged(&entry.manifest_path);
    remove_logged(&entry.document_path);
}

fn remove_logged(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[recovery] could not remove {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
#[path = "recovery_store_tests.rs"]
mod tests;
