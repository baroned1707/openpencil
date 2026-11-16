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
use std::path::PathBuf;

/// Cheap, thread-safe set of open document paths. Paths are stored as the host
/// reports them and canonicalized only when compared (see
/// `mcp_serve::file_path`), so the UI thread never touches the filesystem.
#[derive(Debug, Clone, Default)]
pub(super) struct OpenDocumentPaths(Arc<Mutex<Vec<PathBuf>>>);

impl OpenDocumentPaths {
    /// Replace the set. Skips the write (and the lock contention it implies)
    /// when nothing changed, since the host calls this on every pump.
    pub(super) fn set(&self, paths: &[PathBuf]) {
        let mut guard = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if guard.as_slice() != paths {
            *guard = paths.to_vec();
        }
    }

    pub(super) fn snapshot(&self) -> Vec<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}
