//! Keyboard editing for the Projects dock's inline file rename.
//!
//! Same shape as [`crate::save_name_keyboard`]: hosts route platform key
//! events here so selection / UTF-8 caret semantics stay shared. While a rename
//! is open it owns every keystroke, including the ones it rejects, so a stray
//! `r` cannot reach the rectangle tool and Backspace cannot delete the
//! selected layer while the user is naming a file.
//!
//! Each function returns `None` when no rename is open (the host keeps routing
//! the key) and `Some(changed)` when the rename owned it.

use jian_core::text_input::TextInputState;

use crate::project_rename::RenameConfirm;
use crate::EditorState;

fn edit(state: &mut EditorState) -> Option<&mut crate::ProjectRenameState> {
    state.editor_ui.projects_panel.rename.as_mut()
}

/// Whether a rename is open and so owns the keyboard.
pub fn is_active(state: &EditorState) -> bool {
    state.editor_ui.projects_panel.rename.is_some()
}

/// Insert one printable character. Control characters are dropped here; every
/// other rule is checked live and shown under the field.
pub fn text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    let session = edit(state)?;
    if c.is_control() {
        return Some(false);
    }
    let mut encoded = [0_u8; 4];
    session
        .input
        .insert_str(c.encode_utf8(&mut encoded), now_ms);
    state.editor_ui.projects_panel.refresh_rename_error();
    Some(true)
}

/// Insert clipboard text on one line: control characters and line breaks are
/// dropped.
pub fn paste(state: &mut EditorState, text: &str, now_ms: u64) -> Option<bool> {
    let session = edit(state)?;
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    if cleaned.is_empty() {
        return Some(false);
    }
    session.input.insert_str(&cleaned, now_ms);
    state.editor_ui.projects_panel.refresh_rename_error();
    Some(true)
}

/// Delete the previous character (or the selection).
pub fn backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    edit_with(state, |input| input.backspace(now_ms))
}

/// Delete the next character (or the selection).
pub fn delete_forward(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    edit_with(state, |input| input.delete_forward(now_ms))
}

/// Move the caret by one character; `extend` grows the selection. `false`
/// when no rename is open.
pub fn move_caret(state: &mut EditorState, forward: bool, extend: bool, now_ms: u64) -> bool {
    let Some(session) = edit(state) else {
        return false;
    };
    if forward {
        session.input.move_right(extend, now_ms);
    } else {
        session.input.move_left(extend, now_ms);
    }
    true
}

/// Move the caret to the start or the end; `extend` grows the selection.
pub fn move_caret_to_edge(state: &mut EditorState, end: bool, extend: bool, now_ms: u64) -> bool {
    let Some(session) = edit(state) else {
        return false;
    };
    if end {
        session.input.move_end(extend, now_ms);
    } else {
        session.input.move_home(extend, now_ms);
    }
    true
}

/// Select the whole name.
pub fn select_all(state: &mut EditorState, now_ms: u64) -> bool {
    let Some(session) = edit(state) else {
        return false;
    };
    session.input.select_all();
    session.input.touch(now_ms);
    true
}

/// Enter: confirm the draft. `None` when no rename is open.
pub fn confirm(state: &mut EditorState) -> Option<RenameConfirm> {
    match state.editor_ui.projects_panel.confirm_rename() {
        RenameConfirm::NotEditing => None,
        outcome => Some(outcome),
    }
}

/// Escape: drop the edit. `true` when one was open.
pub fn cancel(state: &mut EditorState) -> bool {
    state.editor_ui.projects_panel.cancel_rename()
}

/// The selected text, for Copy / Cut.
pub fn selected_text(state: &EditorState) -> Option<String> {
    let input = state
        .editor_ui
        .projects_panel
        .rename
        .as_ref()?
        .input
        .clone();
    let (start, end) = input.highlight_range()?;
    input.text().get(start..end).map(str::to_owned)
}

fn edit_with(state: &mut EditorState, change: impl FnOnce(&mut TextInputState)) -> Option<bool> {
    let session = edit(state)?;
    let before = session.input.text().to_owned();
    let selection_before = session.input.highlight_range();
    change(&mut session.input);
    let changed =
        session.input.text() != before || session.input.highlight_range() != selection_before;
    state.editor_ui.projects_panel.refresh_rename_error();
    Some(changed)
}

#[cfg(test)]
#[path = "project_rename_keyboard_tests.rs"]
mod tests;
