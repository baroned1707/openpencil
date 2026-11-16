//! Save a document to a path that must NOT exist yet.
//!
//! `save_to_path` publishes with a rename, which silently replaces whatever is
//! at the destination. A caller creating a brand-new file (the MCP
//! `create_document` tool) must never overwrite one, so this writes the same
//! canonical bytes to a sibling temp file and then publishes it with a hard
//! link, which the OS refuses when the destination exists. The temp file is
//! removed on every path, so a crash or a failure leaves no half-written
//! document under the final name.

use std::path::Path;

use op_editor_core::EditorState;

use super::atomic_file::{create_sibling_temp, replace_file};
use super::{canonical_save, DocIoError};

/// Why a new document could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveNewError {
    /// Something already exists at the destination; nothing was written there.
    AlreadyExists,
    /// Serializing or publishing the document failed.
    Io(DocIoError),
}

/// Write `state`'s document (with its editor metadata and image thumbnails,
/// exactly as Save does) to `path`, refusing to replace an existing entry.
pub fn save_new_to_path(state: &EditorState, path: &Path) -> Result<(), SaveNewError> {
    let thumbnails = jian_ops_schema::image_thumbs::capture_snapshot();
    let (tmp, file) = create_sibling_temp(path).map_err(SaveNewError::Io)?;
    let result = (|| {
        let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
        canonical_save::write_canonical_document_with_thumbnails(
            &mut writer,
            &state.doc,
            op_pen_loader::EditorMeta::from_state(state),
            &thumbnails,
        )
        .map_err(SaveNewError::Io)?;
        std::io::Write::flush(&mut writer)
            .map_err(|error| SaveNewError::Io(DocIoError::Io(error.to_string())))?;
        drop(writer);
        publish_without_replacing(&tmp, path)
    })();
    // The hard link leaves the temp name behind on success; on failure it is
    // the only artifact. Either way it must go.
    let _ = std::fs::remove_file(&tmp);
    result
}

fn publish_without_replacing(tmp: &Path, path: &Path) -> Result<(), SaveNewError> {
    match std::fs::hard_link(tmp, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(SaveNewError::AlreadyExists)
        }
        Err(_) => {
            // The file system may not support hard links (FAT, some network
            // shares). Fall back to a checked rename: there is a short window
            // in which a file could appear, but the entry is still refused
            // when it is already there.
            if std::fs::symlink_metadata(path).is_ok() {
                return Err(SaveNewError::AlreadyExists);
            }
            replace_file(tmp, path).map_err(SaveNewError::Io)
        }
    }
}
