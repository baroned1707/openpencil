//! Opening an EMPTY `.op` file: format it into a blank document.
//!
//! People create an empty `.op` by hand (`touch design.op`) and then open it.
//! An empty file is not a document, so the normal loader rejects it; the user
//! expects to start designing instead. [`load_initializing_empty_op_file`] is
//! the one opt-in funnel for that: when the target is an empty `.op` file it
//! yields the same blank document New does (`EditorState::starter()`) and
//! writes the canonical blank `.op` content into the SAME path, so the file is
//! a valid document immediately.
//!
//! What counts as empty, and nothing else: a regular file with the `.op`
//! extension (case-insensitive) whose content is zero bytes, or only
//! Unicode whitespace after one optional UTF-8 BOM. A file that merely fails to
//! parse (garbage, `{}`, a truncated document) is NEVER touched; it keeps the
//! loader's error. Other extensions (`.pen`, `.fig`, ...) keep today's behaviour.
//!
//! The write never replaces content that appeared in the meantime: the blank is
//! serialised to a sibling temp file first, emptiness is checked again right
//! before the rename, and a file that gained content is loaded instead.
//!
//! The existing loaders are deliberately unchanged: render / enrich CLIs, the
//! web daemon, recovery snapshots and Git reloads must not write to a file
//! they were only asked to read.

use std::io::Read;
use std::path::Path;

use jian_ops_schema::image_thumbs::ImageThumbSnapshot;
use op_editor_core::EditorState;

use super::atomic_file::create_sibling_temp;
use super::{
    canonical_save, commit_staged_document, load_editor_state_with_report, DocIoError,
    DocumentLoadReport, LoadedEditorState,
};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
/// A whitespace-only file larger than this is not treated as empty: nobody
/// creates one by hand, and it bounds the read.
const MAX_EMPTY_FILE_BYTES: u64 = 1 << 20;
/// Bytes inspected first: a real document shows a non-whitespace ASCII byte
/// here, so the common open never reads the file twice.
const QUICK_REJECT_BYTES: usize = 256;

/// What happened to an empty file that was opened as a blank document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmptyFileOutcome {
    /// The blank `.op` content is on disk; the document is saved.
    Formatted,
    /// The blank document could not be written (read-only file or folder,
    /// full disk). The document is still usable in memory and unsaved.
    NotWritable(DocIoError),
}

/// A document opened through [`load_initializing_empty_op_file`].
pub struct OpenedEditorState {
    pub loaded: LoadedEditorState,
    /// `Some` only when the file was empty and was opened as a blank document.
    pub empty_file: Option<EmptyFileOutcome>,
}

/// Whether `path` is an empty `.op` file (see the module docs for the rule).
pub fn is_empty_op_file(path: &Path) -> bool {
    has_op_extension(path) && file_is_blank(path)
}

/// Whether `bytes` are empty or only whitespace after an optional UTF-8 BOM.
pub fn bytes_are_blank(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    std::str::from_utf8(text).is_ok_and(|text| text.chars().all(char::is_whitespace))
}

fn has_op_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("op"))
}

fn file_is_blank(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    // `File::open` succeeds on a directory on Unix, so the kind matters.
    if !metadata.is_file() || metadata.len() > MAX_EMPTY_FILE_BYTES {
        return false;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut head = [0u8; QUICK_REJECT_BYTES];
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(_) => return false,
        }
    }
    // An ASCII byte that is not whitespace (NUL included) is content.
    if head[..filled]
        .iter()
        .any(|byte| byte.is_ascii() && !char::from(*byte).is_whitespace())
    {
        return false;
    }
    bytes.extend_from_slice(&head[..filled]);
    if file
        .take(MAX_EMPTY_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return false;
    }
    bytes.len() as u64 <= MAX_EMPTY_FILE_BYTES && bytes_are_blank(&bytes)
}

/// Load `path` like [`load_editor_state_with_report`], except that an empty
/// `.op` file opens as the blank starter document and is formatted on disk.
pub fn load_initializing_empty_op_file(
    path: &Path,
    locale: op_editor_core::Locale,
) -> Result<OpenedEditorState, DocIoError> {
    load_initializing_empty_op_file_with(path, locale, || {})
}

/// [`load_initializing_empty_op_file`] with a hook that runs after the blank is
/// serialised and before the emptiness re-check; tests use it to play the part
/// of another program writing into the file.
pub(super) fn load_initializing_empty_op_file_with(
    path: &Path,
    locale: op_editor_core::Locale,
    before_publish: impl FnOnce(),
) -> Result<OpenedEditorState, DocIoError> {
    if !is_empty_op_file(path) {
        return load_editor_state_with_report(path, locale).map(|loaded| OpenedEditorState {
            loaded,
            empty_file: None,
        });
    }
    // Built here, not by the caller, so a caller that brackets the load (the
    // desktop does, for the thumbnail registry) brackets the blank as well.
    let blank = EditorState::starter();
    match write_blank_over_empty_file(path, &blank, before_publish) {
        Initialization::Formatted => Ok(blank_opened(blank, EmptyFileOutcome::Formatted)),
        Initialization::NotWritable(error) => {
            eprintln!(
                "[open] empty file {} could not be formatted: {error}",
                path.display()
            );
            Ok(blank_opened(blank, EmptyFileOutcome::NotWritable(error)))
        }
        // Something wrote real content between the check and the write: that
        // content is the document, the blank is discarded.
        Initialization::GainedContent => {
            load_editor_state_with_report(path, locale).map(|loaded| OpenedEditorState {
                loaded,
                empty_file: None,
            })
        }
    }
}

fn blank_opened(state: EditorState, outcome: EmptyFileOutcome) -> OpenedEditorState {
    OpenedEditorState {
        loaded: LoadedEditorState {
            state,
            report: DocumentLoadReport::default(),
        },
        empty_file: Some(outcome),
    }
}

enum Initialization {
    Formatted,
    GainedContent,
    NotWritable(DocIoError),
}

/// Serialise `blank` next to `path`, re-check that `path` is still empty, then
/// atomically rename the temp file over it. The blank carries no image
/// thumbnails of its own, so none are written (the process-wide registry
/// belongs to whichever document is on screen).
fn write_blank_over_empty_file(
    path: &Path,
    blank: &EditorState,
    before_publish: impl FnOnce(),
) -> Initialization {
    // Write to the real file, not over a symlink that points at it.
    let target = match path.canonicalize() {
        Ok(target) => target,
        Err(error) => return Initialization::NotWritable(DocIoError::Io(error.to_string())),
    };
    // A rename succeeds over a read-only file, so honour the permission here.
    match std::fs::metadata(&target) {
        Ok(metadata) if metadata.permissions().readonly() => {
            return Initialization::NotWritable(DocIoError::Io(format!(
                "{} is read-only",
                target.display()
            )))
        }
        Ok(_) => {}
        Err(error) => return Initialization::NotWritable(DocIoError::Io(error.to_string())),
    }
    let (temp, file) = match create_sibling_temp(&target) {
        Ok(created) => created,
        Err(error) => return Initialization::NotWritable(error),
    };
    let published = (|| {
        let mut writer = std::io::BufWriter::new(file);
        canonical_save::write_canonical_document_with_thumbnails(
            &mut writer,
            &blank.doc,
            op_pen_loader::EditorMeta::from_state(blank),
            &ImageThumbSnapshot::default(),
        )?;
        std::io::Write::flush(&mut writer).map_err(|error| DocIoError::Io(error.to_string()))?;
        drop(writer);
        before_publish();
        if !file_is_blank(&target) {
            return Ok(false);
        }
        commit_staged_document(&temp, &target)?;
        Ok(true)
    })();
    // After a successful rename the temp name is gone; otherwise it is the
    // only artifact. Removing it on every path leaves no half file behind.
    let _ = std::fs::remove_file(&temp);
    match published {
        Ok(true) => Initialization::Formatted,
        Ok(false) => Initialization::GainedContent,
        Err(error) => Initialization::NotWritable(error),
    }
}

#[cfg(test)]
#[path = "empty_file_tests.rs"]
mod tests;
