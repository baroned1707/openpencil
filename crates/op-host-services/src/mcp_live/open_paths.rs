//! The files the editor currently has open, shared with the connection
//! threads so a `filePath` tool argument can be checked against them.
//!
//! A tool call that names a file on disk is served by loading that file,
//! editing it and writing it back. When that file is the document open in the
//! editor, the write would land behind the editor's back and the next Save
//! would overwrite it, so the connection refuses the call instead. The UI
//! thread publishes the paths from `McpLiveServer::sync_host`; connection
//! threads only read them.

use super::*;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
struct OpenState {
    /// Every open document's file, on screen or parked.
    paths: Vec<PathBuf>,
    /// The on-screen document's file; `None` while it is untitled.
    active: Option<PathBuf>,
}

/// Cheap, thread-safe set of open document paths. Paths are stored as the host
/// reports them and canonicalized only when compared (see
/// `mcp_serve::file_path`), so the UI thread never touches the filesystem.
#[derive(Debug, Clone, Default)]
pub(super) struct OpenDocumentPaths(Arc<Mutex<OpenState>>);

impl OpenDocumentPaths {
    fn lock(&self) -> std::sync::MutexGuard<'_, OpenState> {
        self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    /// Replace the set. Skips the write (and the lock contention it implies)
    /// when nothing changed, since the host calls this on every pump.
    pub(super) fn set(&self, paths: &[PathBuf]) {
        let mut guard = self.lock();
        if guard.paths.as_slice() != paths {
            guard.paths = paths.to_vec();
        }
    }

    /// Replace the on-screen document's file (`None` for an untitled one).
    pub(super) fn set_active(&self, active: Option<&Path>) {
        let mut guard = self.lock();
        if guard.active.as_deref() != active {
            guard.active = active.map(Path::to_path_buf);
        }
    }

    pub(super) fn snapshot(&self) -> Vec<PathBuf> {
        self.lock().paths.clone()
    }

    /// The on-screen document's file, if it has one.
    pub(super) fn active(&self) -> Option<PathBuf> {
        self.lock().active.clone()
    }

    /// Record that the on-screen document was just given `path`, ahead of the
    /// host's next `sync_host` (which will report the same thing).
    pub(super) fn bind(&self, path: &Path) {
        let mut guard = self.lock();
        if !guard.paths.iter().any(|open| open == path) {
            guard.paths.push(path.to_path_buf());
        }
        guard.active = Some(path.to_path_buf());
    }
}
