//! When a user save would replace a file the editor cannot vouch for.
//!
//! Cmd+S, the Save button of the close / quit prompts and Save As onto an
//! existing file all end in an atomic replace. If the file changed on disk
//! since the editor last read or wrote it (a `git checkout`, another editor, a
//! second instance), or the editor never recorded what it looked like, the
//! replace would silently destroy someone else's version. This module holds the
//! pure decision and the question put to the user; `save_conflict_host.rs`
//! wires it into the save flow.

use std::path::{Path, PathBuf};

use op_host_services::disk_fingerprint::DiskFingerprint;

/// Why the editor will not overwrite without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveConflict {
    /// The file differs from what the editor last read or wrote.
    ChangedOnDisk,
    /// The editor holds no usable record of the file: never recorded, marked
    /// stale (a recovered document), or the file could not be examined.
    Unconfirmed,
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictChoice {
    Overwrite,
    SaveCopy,
    Cancel,
}

/// Whether saving to `path` needs the user's say, given the editor's record of
/// the file. A file that is gone is simply recreated (nothing to destroy).
pub(crate) fn classify(baseline: Option<DiskFingerprint>, path: &Path) -> Option<SaveConflict> {
    let current = match DiskFingerprint::capture(path) {
        Ok(current) => current,
        Err(_) => return Some(SaveConflict::Unconfirmed),
    };
    if current == DiskFingerprint::Missing {
        return None;
    }
    match baseline {
        None | Some(DiskFingerprint::Stale) => Some(SaveConflict::Unconfirmed),
        Some(known) if known == current => None,
        Some(_) => Some(SaveConflict::ChangedOnDisk),
    }
}

/// `<stem>-copy.<ext>` beside `path`, or `<stem>-copy-2.<ext>` ... when taken:
/// a name that does not exist yet, so the copy never overwrites anything.
pub(crate) fn unique_copy_path(path: &Path) -> PathBuf {
    let directory = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_else(|| "op".to_string());
    let name_for = |attempt: u32| match attempt {
        1 => format!("{stem}-copy.{extension}"),
        n => format!("{stem}-copy-{n}.{extension}"),
    };
    (1..10_000)
        .map(|attempt| directory.join(name_for(attempt)))
        .find(|candidate| std::fs::symlink_metadata(candidate).is_err())
        .unwrap_or_else(|| {
            directory.join(format!("{stem}-copy-{}.{extension}", std::process::id()))
        })
}

/// How the question is asked; the desktop uses native dialogs, tests inject
/// the answers.
pub(crate) trait ConflictUi {
    fn ask(
        &mut self,
        locale: op_editor_core::Locale,
        name: &str,
        conflict: SaveConflict,
    ) -> ConflictChoice;
    /// The Save As picker for a copy, pre-filled with `default`.
    fn pick_copy_path(
        &mut self,
        state: &op_editor_core::EditorState,
        default: &Path,
    ) -> Option<PathBuf>;
}

/// Native dialogs. Yes = Overwrite, No = Save a copy, Cancel = Cancel (the
/// body says so, in the user's language). A dialog that cannot be shown
/// (`None`: Linux with no zenity / kdialog) cancels: unlike the unsaved-changes
/// prompts, falling back to "save anyway" here would destroy the other version.
#[cfg_attr(test, allow(dead_code))]
pub(crate) struct NativeConflictUi;

impl ConflictUi for NativeConflictUi {
    fn ask(
        &mut self,
        locale: op_editor_core::Locale,
        name: &str,
        conflict: SaveConflict,
    ) -> ConflictChoice {
        let body_key = match conflict {
            SaveConflict::ChangedOnDisk => "dialog.saveConflictChangedBody",
            SaveConflict::Unconfirmed => "dialog.saveConflictUnknownBody",
        };
        let body = op_i18n::translate(locale, body_key).replace("{{name}}", name);
        match crate::message_dialog::ask_yes_no_cancel(
            op_i18n::translate(locale, "dialog.saveConflictTitle"),
            &body,
            rfd::MessageLevel::Warning,
        ) {
            Some(crate::message_dialog::Choice::Yes) => ConflictChoice::Overwrite,
            Some(crate::message_dialog::Choice::No) => ConflictChoice::SaveCopy,
            Some(crate::message_dialog::Choice::Cancel) | None => ConflictChoice::Cancel,
        }
    }

    fn pick_copy_path(
        &mut self,
        state: &op_editor_core::EditorState,
        default: &Path,
    ) -> Option<PathBuf> {
        crate::persistence::pick_save_copy_path(state, default)
    }
}

/// What tests that did not inject an answer get: cancel, loudly, so a save
/// that unexpectedly needs a prompt fails the test instead of opening a dialog.
#[cfg(test)]
pub(crate) struct RefusingConflictUi;

#[cfg(test)]
impl ConflictUi for RefusingConflictUi {
    fn ask(
        &mut self,
        _: op_editor_core::Locale,
        name: &str,
        conflict: SaveConflict,
    ) -> ConflictChoice {
        eprintln!("[test] unexpected save-conflict prompt for {name}: {conflict:?}");
        ConflictChoice::Cancel
    }

    fn pick_copy_path(&mut self, _: &op_editor_core::EditorState, _: &Path) -> Option<PathBuf> {
        None
    }
}

/// Answers Overwrite to every question and counts them, for tests of flows
/// where the first save after a recovery is expected to ask.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct OverwritingConflictUi {
    pub(crate) asked: usize,
}

#[cfg(test)]
impl ConflictUi for OverwritingConflictUi {
    fn ask(&mut self, _: op_editor_core::Locale, _: &str, _: SaveConflict) -> ConflictChoice {
        self.asked += 1;
        ConflictChoice::Overwrite
    }

    fn pick_copy_path(&mut self, _: &op_editor_core::EditorState, _: &Path) -> Option<PathBuf> {
        None
    }
}

#[cfg(not(test))]
pub(crate) fn default_ui() -> NativeConflictUi {
    NativeConflictUi
}

#[cfg(test)]
pub(crate) fn default_ui() -> RefusingConflictUi {
    RefusingConflictUi
}
