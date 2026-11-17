//! What each open document's file looked like when the editor last read or
//! wrote it (`DiskFingerprint`), so an agent's in-place save and the autosave
//! at the end of a run can refuse to replace a file somebody else changed.
//!
//! The map is keyed by the file path, not by document: a Cmd+S completion
//! knows the path it wrote, and a Save As or a tab rename simply produces a
//! path with no entry, which the next sweep records. A sweep also drops the
//! entry of every file no tab holds any more, so a file closed and opened again
//! later starts from a fresh baseline instead of the old one.
//!
//! The first sight of a path (the sweep runs once per frame, so within a frame
//! of the file being opened) records the file as it is then. Only paths
//! without an entry cost a `stat`; an idle frame does no file-system work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use op_host_services::disk_fingerprint::DiskFingerprint;

use crate::DesktopApp;

/// The recorded on-disk state of every file an open tab is bound to.
#[derive(Default)]
pub(crate) struct DiskBaselines {
    entries: HashMap<PathBuf, DiskFingerprint>,
}

impl DiskBaselines {
    /// Drop the entries of files no tab holds, and record the files that have
    /// none yet. `open` is every path some tab is bound to.
    pub(crate) fn sweep(&mut self, open: &[PathBuf]) {
        self.entries.retain(|path, _| open.contains(path));
        for path in open {
            if !self.entries.contains_key(path) {
                if let Ok(fingerprint) = DiskFingerprint::capture(path) {
                    self.entries.insert(path.clone(), fingerprint);
                }
            }
        }
    }

    /// The editor just wrote `path`: this is what it left there.
    pub(crate) fn record(&mut self, path: &Path, fingerprint: DiskFingerprint) {
        self.entries.insert(path.to_path_buf(), fingerprint);
    }

    /// [`Self::record`] for a write whose result is read back from disk.
    pub(crate) fn record_from_disk(&mut self, path: &Path) {
        match DiskFingerprint::capture(path) {
            Ok(fingerprint) => self.record(path, fingerprint),
            // Unknown state: forget it, so the next in-place save refuses
            // instead of trusting a stale record.
            Err(_) => {
                self.entries.remove(path);
            }
        }
    }

    pub(crate) fn get(&self, path: &Path) -> Option<DiskFingerprint> {
        self.entries.get(path).copied()
    }
}

impl DesktopApp {
    /// Record the files of newly opened tabs and forget closed ones. Cheap
    /// when nothing changed; call it once per frame before anything that may
    /// save a document in place.
    pub(crate) fn sweep_disk_baselines(&mut self) {
        self.sync_active_slot();
        let open: Vec<PathBuf> = self.documents.slot_paths().into_iter().flatten().collect();
        self.disk_baselines.sweep(&open);
    }
}

#[cfg(test)]
#[path = "disk_baseline_host_tests.rs"]
mod tests;
