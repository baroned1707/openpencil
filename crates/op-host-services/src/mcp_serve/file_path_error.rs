//! Typed refusal for a per-call `filePath` that names a document the host
//! already has open (see [`super::file_path`]).
//!
//! Kept apart from [`super::McpServeError`]: this is a tool-level refusal the
//! client reads as an `isError` result, never a transport fault, so it must
//! not widen the error table the web daemon maps exhaustively.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilePathError {
    /// `path` (as the caller wrote it) resolves to a file the editor has open.
    /// Serving the call would edit it on disk behind the editor's back, and
    /// the next Save would overwrite the edit.
    OpenInEditor { path: PathBuf },
}

impl fmt::Display for FilePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilePathError::OpenInEditor { path } => write!(
                f,
                "{} is the document currently open in the editor; omit filePath to edit the live canvas instead",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FilePathError {}
