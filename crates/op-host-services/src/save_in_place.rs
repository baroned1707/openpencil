//! Save a document to its OWN file, without a dialog: the Cmd+S equivalent for
//! callers that are not the user (the MCP `save_document` without a
//! `filePath`, and the autosave when an agent run ends).
//!
//! A `save_document` whose `filePath` names the document's OWN file is the same
//! save (`own_path_claim`): the file is the document's, so there is no copy to
//! write and nothing for the path policy to guard.
//!
//! One routine serves all of them, so they cannot drift: refuse a document with no
//! file, refuse a collaboration session that does not let this save happen,
//! refuse when the file changed on disk since the editor last saw it (an
//! agent's save must never silently replace someone else's change), then write
//! through `doc_io::save_to_path` (serialise to a sibling temp file, fsync,
//! rename over the target, the same commit the Cmd+S worker uses) and only
//! then mark the document saved. A failed write leaves the old file AND the
//! dirty mark untouched.

use std::fmt;
use std::path::{Path, PathBuf};

use op_editor_core::{CollabEditSource, CollabGateAction, CollabGatePolicy, EditorState};

use crate::disk_fingerprint::{check_unchanged, DiskCheckError, DiskFingerprint};
use crate::doc_io::DocIoError;

/// What a successful in-place save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedInPlace {
    pub path: PathBuf,
    /// Size of the file after the write.
    pub bytes: u64,
    /// Whether the document had unsaved changes before this save.
    pub dirty_before: bool,
    /// The file as the editor left it, to become the new baseline.
    pub fingerprint: DiskFingerprint,
}

/// Why nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveInPlaceError {
    /// The document has no file yet.
    Untitled,
    /// A collaboration session does not let this document be saved by an agent.
    CollaborationActive,
    /// A Cmd+S / Save As write is still running; its rename could land after
    /// this one and roll the file back.
    SaveInFlight,
    /// The editor holds no record of the file's state, so a change cannot be
    /// ruled out.
    BaselineUnknown { path: PathBuf },
    /// The file differs from what the editor last read or wrote.
    ChangedOnDisk { path: PathBuf },
    /// The file could not be examined.
    Unreadable { path: PathBuf, detail: String },
    /// The write failed; the old file is intact.
    Write { path: PathBuf, detail: String },
}

impl fmt::Display for SaveInPlaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Untitled => f.write_str(
                "this document has no file yet: call create_document { projectRoot, name } \
                 or save with filePath",
            ),
            Self::CollaborationActive => f.write_str(
                "this document is in a collaboration session; leave the session before an \
                 agent saves it",
            ),
            Self::SaveInFlight => f.write_str(
                "NOT saved: a save of this document is still running; retry in a moment",
            ),
            Self::BaselineUnknown { path } => write!(
                f,
                "NOT saved: the editor has not recorded the state of {} yet; retry in a moment",
                path.display()
            ),
            Self::ChangedOnDisk { path } => write!(
                f,
                "the file changed on disk since it was opened ({}); open it again or use Save As",
                path.display()
            ),
            Self::Unreadable { path, detail } => {
                write!(
                    f,
                    "cannot examine {} before saving: {detail}",
                    path.display()
                )
            }
            Self::Write { path, detail } => write!(
                f,
                "writing {} failed, the file on disk is unchanged: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SaveInPlaceError {}

impl SaveInPlaceError {
    /// A refusal that a retry in a moment may clear, as opposed to one the
    /// caller must act on.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::SaveInFlight | Self::BaselineUnknown { .. })
    }
}

/// The tool this is the in-place form of.
pub const TOOL_NAME: &str = "save_document";

/// Whether `call` is the in-place form: `save_document` that names neither a
/// target nor a source file. Blank counts as absent, as in `path_policy`.
pub fn is_in_place_save_call(call: &op_mcp::ToolCall) -> bool {
    call.tool == TOOL_NAME
        && ["filePath", "sourceFilePath"].iter().all(|key| {
            call.arguments
                .get(*key)
                .is_none_or(|value| value.trim().is_empty())
        })
}

/// The `filePath` of a `save_document` that may name the document's own file,
/// i.e. the call that names a target but no source file (`sourceFilePath`
/// asks for a different operation: copy THAT file). Whether the path really is
/// the document's file is decided where the document's path is known; a path
/// that is not stays an ordinary copy.
pub fn own_path_claim(call: &op_mcp::ToolCall) -> Option<&str> {
    let named = |key: &str| {
        call.arguments
            .get(key)
            .filter(|value| !value.trim().is_empty())
    };
    if call.tool != TOOL_NAME || named("sourceFilePath").is_some() {
        return None;
    }
    named("filePath").map(String::as_str)
}

/// The result an agent reads: `{ saved, path, bytes, documentId, dirtyBefore }`.
pub fn result_json(saved: &SavedInPlace, document_id: &str) -> String {
    serde_json::json!({
        "saved": true,
        "path": saved.path.to_string_lossy(),
        "bytes": saved.bytes,
        "documentId": document_id,
        "dirtyBefore": saved.dirty_before,
    })
    .to_string()
}

/// Save `state` to `path` (its own file; `None` while untitled).
/// `baseline` is what the editor last saw of that file.
pub fn save_in_place(
    state: &mut EditorState,
    path: Option<&Path>,
    baseline: Option<&DiskFingerprint>,
) -> Result<SavedInPlace, SaveInPlaceError> {
    save_in_place_with(state, path, baseline, crate::doc_io::save_to_path)
}

/// [`save_in_place`] for a host that may have a Cmd+S / Save As write still
/// running (`save_in_flight`): its rename could land after this one and roll
/// the file back, so the save is refused (an untitled document is still
/// reported as untitled first).
pub fn save_in_place_unless_busy(
    state: &mut EditorState,
    path: Option<&Path>,
    baseline: Option<&DiskFingerprint>,
    save_in_flight: bool,
) -> Result<SavedInPlace, SaveInPlaceError> {
    if path.is_some() && save_in_flight {
        return Err(SaveInPlaceError::SaveInFlight);
    }
    save_in_place(state, path, baseline)
}

/// [`save_in_place`] writing through `write` (a test's failing writer).
pub fn save_in_place_with(
    state: &mut EditorState,
    path: Option<&Path>,
    baseline: Option<&DiskFingerprint>,
    write: impl FnOnce(&EditorState, &Path) -> Result<(), DocIoError>,
) -> Result<SavedInPlace, SaveInPlaceError> {
    let path = path.ok_or(SaveInPlaceError::Untitled)?;
    if CollabGatePolicy::from(&state.editor_ui.collab)
        .check(CollabGateAction::SaveShared, CollabEditSource::Mcp)
        .is_err()
    {
        return Err(SaveInPlaceError::CollaborationActive);
    }
    let baseline = baseline.ok_or_else(|| SaveInPlaceError::BaselineUnknown {
        path: path.to_path_buf(),
    })?;
    check_unchanged(baseline, path).map_err(|error| match error {
        DiskCheckError::Changed => SaveInPlaceError::ChangedOnDisk {
            path: path.to_path_buf(),
        },
        DiskCheckError::Unreadable(detail) => SaveInPlaceError::Unreadable {
            path: path.to_path_buf(),
            detail,
        },
    })?;
    let dirty_before = state.is_dirty();
    write(state, path).map_err(|error| SaveInPlaceError::Write {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    // Written: now, and only now, the document is the file.
    state.mark_saved_revision();
    crate::doc_io::set_file_name_display(state, Some(path));
    // The file was just written, so a stat failure here is a freak; recording
    // `Missing` makes the NEXT in-place save refuse (fail closed) instead of
    // reporting a written file as unsaved.
    let fingerprint = DiskFingerprint::capture(path).unwrap_or(DiskFingerprint::Missing);
    let bytes = match fingerprint {
        DiskFingerprint::Present { len, .. } => len,
        DiskFingerprint::Missing => 0,
    };
    Ok(SavedInPlace {
        path: path.to_path_buf(),
        bytes,
        dirty_before,
        fingerprint,
    })
}

#[cfg(test)]
#[path = "save_in_place_tests.rs"]
mod tests;
