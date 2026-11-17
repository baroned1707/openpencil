//! Renaming a design file from the Projects dock: the rules for a valid name,
//! the inline edit session and its transitions.
//!
//! The dock only edits a draft. When the user confirms a valid name the edit
//! records a [`ProjectRenameRequest`] in `ProjectsPanelState::rename_commit`
//! (its own slot, NOT `pending_request`: a click-away commit and the press that
//! caused it can land in the same frame, and a single `Option` would lose one).
//! The host drains the slot, re-validates against the disk and answers with
//! [`ProjectsPanelState::finish_rename`] or [`ProjectsPanelState::reject_rename`].
//!
//! Everything here is app-level and keyed by the file's PATH, never by a row
//! index: the host rebuilds the rows between two presses of a double click.
//!
//! The extension is never edited. The field holds the stem; a trailing `.op`
//! typed by the user is dropped, because nobody who types `card.op` means the
//! file `card.op.op`.

use jian_core::text_input::TextInputState;

use crate::editor_ui_state::ProjectsPanelState;
use crate::project_tree::{ProjectFileKind, ProjectFileView, ProjectView};

/// Longest stem (the name without its extension) in bytes.
pub const MAX_STEM_BYTES: usize = 120;

/// Two presses on the same file row within this many ms start a rename (the
/// same window the layer panel and the canvas use).
pub const DOUBLE_CLICK_MS: u64 = 400;

/// Why a draft name cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameNameError {
    /// Nothing left after trimming.
    Empty,
    /// Contains `/` or `\`.
    Separator,
    /// Contains a control character (NUL included).
    Control,
    /// Contains a character Windows file names cannot carry (`< > : " | ? *`).
    ReservedChar,
    /// `.` or `..`.
    DotName,
    /// Starts with a dot (a hidden file the dock would stop listing).
    LeadingDot,
    /// Ends with a dot or a space (Windows trims both).
    TrailingDotOrSpace,
    /// Longer than [`MAX_STEM_BYTES`].
    TooLong,
    /// Another file in the same folder already has this name (compared
    /// ignoring case, because the default macOS volume does).
    Taken,
}

impl RenameNameError {
    /// The message shown under the field.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Taken => "projects.renameExists",
            _ => "projects.renameInvalid",
        }
    }
}

/// The stem a draft stands for: trimmed, with one trailing `.op` dropped.
///
/// Returns the first rule the name breaks. The returned stem is what the host
/// appends the file's own extension to.
pub fn normalized_stem(draft: &str) -> Result<String, RenameNameError> {
    let trimmed = draft.trim();
    let stem = strip_op_extension(trimmed).unwrap_or(trimmed);
    if stem.is_empty() {
        return Err(RenameNameError::Empty);
    }
    if stem.contains(['/', '\\']) {
        return Err(RenameNameError::Separator);
    }
    if stem.chars().any(char::is_control) {
        return Err(RenameNameError::Control);
    }
    if stem.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(RenameNameError::ReservedChar);
    }
    if stem == "." || stem == ".." {
        return Err(RenameNameError::DotName);
    }
    if stem.starts_with('.') {
        return Err(RenameNameError::LeadingDot);
    }
    if stem.ends_with(['.', ' ']) {
        return Err(RenameNameError::TrailingDotOrSpace);
    }
    if stem.len() > MAX_STEM_BYTES {
        return Err(RenameNameError::TooLong);
    }
    Ok(stem.to_string())
}

/// `name` without a trailing `.op` (any case), or `None` when it has none.
fn strip_op_extension(name: &str) -> Option<&str> {
    let split = name.len().checked_sub(3)?;
    let tail = name.get(split..)?;
    tail.eq_ignore_ascii_case(".op").then(|| &name[..split])
}

/// The stem of a design file's name: `None` when the name does not end in
/// `.op`, which is the only kind of file the dock renames.
pub fn stem_of_design_name(file_name: &str) -> Option<&str> {
    strip_op_extension(file_name).filter(|stem| !stem.is_empty())
}

/// Whether the dock offers to rename this row: an `.op` design.
pub fn is_renamable(file: &ProjectFileView) -> bool {
    matches!(file.kind, ProjectFileKind::Design) && stem_of_design_name(&file.label).is_some()
}

/// The inline edit of one file's name.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRenameState {
    /// The file being renamed (its path in the row it was started on).
    pub path: String,
    pub input: TextInputState,
    /// What is wrong with the draft, shown under the field.
    pub error: Option<RenameNameError>,
}

/// A confirmed rename for the host to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRenameRequest {
    /// The file to rename, as listed.
    pub path: String,
    /// The validated new stem; the host re-validates it.
    pub new_stem: String,
}

/// What confirming the draft did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameConfirm {
    /// No edit was open.
    NotEditing,
    /// The draft is not usable: the edit stays open and shows why.
    Rejected,
    /// The name is the one the file already has: the edit closed, nothing to do.
    Unchanged,
    /// A request was recorded for the host.
    Submitted,
}

impl ProjectsPanelState {
    /// The project and file listed at `path`.
    pub fn file_at_path(&self, path: &str) -> Option<(&ProjectView, &ProjectFileView)> {
        self.projects.iter().find_map(|project| {
            project
                .files
                .iter()
                .find(|file| file.path == path)
                .map(|file| (project, file))
        })
    }

    /// Start renaming the design at `path`, with its stem selected. Any edit
    /// already open is dropped first. `false` when the row is gone or is not an
    /// `.op` design (a `.fig` is a source, not renamable).
    pub fn begin_rename(&mut self, path: &str, now_ms: u64) -> bool {
        let Some((_, file)) = self.file_at_path(path) else {
            return false;
        };
        if !is_renamable(file) {
            return false;
        }
        let stem = stem_of_design_name(&file.label).unwrap_or_default();
        let mut input = TextInputState::with_text(stem);
        input.select_all();
        input.touch(now_ms);
        self.rename = Some(ProjectRenameState {
            path: path.to_string(),
            input,
            error: None,
        });
        true
    }

    /// Re-check the draft after an edit. An empty draft stays quiet until the
    /// user confirms it.
    pub fn refresh_rename_error(&mut self) {
        let error =
            self.rename
                .as_ref()
                .and_then(|edit| match normalized_stem(edit.input.text()) {
                    Ok(stem) => self
                        .stem_is_taken(&edit.path, &stem)
                        .then_some(RenameNameError::Taken),
                    Err(RenameNameError::Empty) => None,
                    Err(error) => Some(error),
                });
        if let Some(edit) = self.rename.as_mut() {
            edit.error = error;
        }
    }

    /// Whether another file in the same folder of the same project already has
    /// `stem` (ignoring case), next to the file at `path`.
    fn stem_is_taken(&self, path: &str, stem: &str) -> bool {
        let Some((project, file)) = self.file_at_path(path) else {
            return false;
        };
        let extension = file
            .label
            .len()
            .checked_sub(3)
            .and_then(|at| file.label.get(at..))
            .unwrap_or(".op");
        let wanted = format!("{stem}{extension}").to_lowercase();
        project.files.iter().any(|other| {
            other.path != path
                && other.folder == file.folder
                && other.label.to_lowercase() == wanted
        })
    }

    /// Confirm the draft (Enter). A usable new name becomes a request for the
    /// host and the edit stays open until the host answers; an unusable one
    /// keeps the edit open with the reason; the unchanged name just closes it.
    pub fn confirm_rename(&mut self) -> RenameConfirm {
        let Some(edit) = self.rename.as_ref() else {
            return RenameConfirm::NotEditing;
        };
        let path = edit.path.clone();
        let stem = match normalized_stem(edit.input.text()) {
            Ok(stem) => stem,
            Err(error) => {
                self.set_rename_error(Some(error));
                return RenameConfirm::Rejected;
            }
        };
        let current = self
            .file_at_path(&path)
            .and_then(|(_, file)| stem_of_design_name(&file.label));
        if current == Some(stem.as_str()) {
            self.rename = None;
            return RenameConfirm::Unchanged;
        }
        if self.stem_is_taken(&path, &stem) {
            self.set_rename_error(Some(RenameNameError::Taken));
            return RenameConfirm::Rejected;
        }
        self.set_rename_error(None);
        self.rename_commit = Some(ProjectRenameRequest {
            path,
            new_stem: stem,
        });
        RenameConfirm::Submitted
    }

    /// Close the edit without renaming (Escape). `true` when one was open.
    pub fn cancel_rename(&mut self) -> bool {
        self.rename.take().is_some()
    }

    /// A press outside the field: keep a valid new name, drop anything else.
    /// Returns whether an edit was open.
    pub fn commit_or_cancel_rename(&mut self) -> bool {
        match self.confirm_rename() {
            RenameConfirm::NotEditing => false,
            RenameConfirm::Rejected => {
                self.rename = None;
                true
            }
            RenameConfirm::Unchanged | RenameConfirm::Submitted => true,
        }
    }

    /// The host renamed the file (or found nothing left to do): close the edit.
    pub fn finish_rename(&mut self, path: &str) {
        if self.rename.as_ref().is_some_and(|edit| edit.path == path) {
            self.rename = None;
        }
    }

    /// The host refused the name: keep the edit open and say why.
    pub fn reject_rename(&mut self, path: &str, error: RenameNameError) {
        if let Some(edit) = self.rename.as_mut().filter(|edit| edit.path == path) {
            edit.error = Some(error);
        }
    }

    fn set_rename_error(&mut self, error: Option<RenameNameError>) {
        if let Some(edit) = self.rename.as_mut() {
            edit.error = error;
        }
    }

    /// Whether the row being renamed is on screen: its project expanded, its
    /// folder unfolded and the dock open.
    pub fn rename_row_visible(&self) -> bool {
        let Some(edit) = self.rename.as_ref() else {
            return false;
        };
        let Some((project, file)) = self.file_at_path(&edit.path) else {
            return false;
        };
        self.is_open()
            && project.expanded
            && (file.folder.is_empty() || !project.is_folder_collapsed(&file.folder))
    }

    /// Drop an edit whose row can no longer be seen (the file vanished, its
    /// folder was folded, the dock closed), so an invisible field never keeps
    /// the keyboard. Returns whether one was dropped.
    pub fn drop_hidden_rename(&mut self) -> bool {
        if self.rename.is_some() && !self.rename_row_visible() {
            self.rename = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "project_rename_tests.rs"]
mod tests;
