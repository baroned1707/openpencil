//! Keyboard and pointer transitions of the quick switcher.
//!
//! Same shape as [`crate::project_rename_keyboard`]: hosts route platform key
//! events here so the query's caret and selection semantics stay shared. While
//! the switcher is open it owns every keystroke, so a stray letter cannot reach
//! a tool shortcut and Enter cannot send the chat.
//!
//! Each function returns `None` / `false` when the switcher is closed (the host
//! keeps routing the key) and `Some(changed)` / `true` when it owned the key.
//! Choosing a row never touches a document here: it records the request the
//! host already knows how to run (`pending_document_request` for an open
//! document, `projects_panel.pending_request` for a file).

use crate::document_tabs::DocumentTabRequest;
use crate::project_tree::ProjectRequest;
use crate::quick_switcher::{QuickSwitcherRow, QuickSwitcherState, QuickSwitcherTarget};
use crate::EditorState;

fn switcher(state: &mut EditorState) -> Option<&mut QuickSwitcherState> {
    Some(&mut state.editor_ui.projects_panel.switcher).filter(|switcher| switcher.open)
}

/// Whether the switcher is open and so owns the keyboard.
pub fn is_active(state: &EditorState) -> bool {
    state.editor_ui.projects_panel.switcher.open
}

/// The rows for the current query.
pub fn rows(state: &EditorState) -> Vec<QuickSwitcherRow> {
    let ui = &state.editor_ui;
    ui.projects_panel
        .switcher
        .rows(&ui.document_tabs, &ui.projects_panel.projects)
}

/// Open the switcher. `false` (and nothing changes) on a host that holds one
/// document, or while the touch chrome is up.
pub fn open(state: &mut EditorState, now_ms: u64) -> bool {
    if !state.editor_ui.projects_dock_available() {
        return false;
    }
    state.editor_ui.projects_panel.switcher.open_fresh(now_ms);
    true
}

/// Close the switcher. `true` when it was open.
pub fn close(state: &mut EditorState) -> bool {
    state.editor_ui.projects_panel.switcher.close()
}

/// Insert one printable character into the query.
pub fn text(state: &mut EditorState, c: char, now_ms: u64) -> Option<bool> {
    let session = switcher(state)?;
    if c.is_control() {
        return Some(false);
    }
    let mut encoded = [0_u8; 4];
    session
        .input
        .insert_str(c.encode_utf8(&mut encoded), now_ms);
    session.query_changed();
    Some(true)
}

/// Insert clipboard text on one line.
pub fn paste(state: &mut EditorState, text: &str, now_ms: u64) -> Option<bool> {
    let session = switcher(state)?;
    let cleaned: String = text.chars().filter(|c| !c.is_control()).collect();
    if cleaned.is_empty() {
        return Some(false);
    }
    session.input.insert_str(&cleaned, now_ms);
    session.query_changed();
    Some(true)
}

fn edit_with(
    state: &mut EditorState,
    change: impl FnOnce(&mut jian_core::text_input::TextInputState),
) -> Option<bool> {
    let session = switcher(state)?;
    let before = session.input.text().to_owned();
    change(&mut session.input);
    let changed = session.input.text() != before;
    if changed {
        session.query_changed();
    }
    Some(changed)
}

/// Delete the previous character (or the selection).
pub fn backspace(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    edit_with(state, |input| input.backspace(now_ms))
}

/// Delete the next character (or the selection).
pub fn delete_forward(state: &mut EditorState, now_ms: u64) -> Option<bool> {
    edit_with(state, |input| input.delete_forward(now_ms))
}

/// Move the query's caret one character. `false` when the switcher is closed.
pub fn move_caret(state: &mut EditorState, forward: bool, extend: bool, now_ms: u64) -> bool {
    let Some(session) = switcher(state) else {
        return false;
    };
    if forward {
        session.input.move_right(extend, now_ms);
    } else {
        session.input.move_left(extend, now_ms);
    }
    true
}

/// Move the query's caret to the start or the end.
pub fn move_caret_to_edge(state: &mut EditorState, end: bool, extend: bool, now_ms: u64) -> bool {
    let Some(session) = switcher(state) else {
        return false;
    };
    if end {
        session.input.move_end(extend, now_ms);
    } else {
        session.input.move_home(extend, now_ms);
    }
    true
}

/// Select the whole query.
pub fn select_all(state: &mut EditorState, now_ms: u64) -> bool {
    let Some(session) = switcher(state) else {
        return false;
    };
    session.input.select_all();
    session.input.touch(now_ms);
    true
}

/// Move the highlighted row up or down, wrapping. `false` when closed.
pub fn move_selection(state: &mut EditorState, delta: isize) -> bool {
    let count = rows(state).len();
    let Some(session) = switcher(state) else {
        return false;
    };
    session.move_selection(delta, count);
    true
}

/// Highlight `index` (a pointer hovering the row). `true` when it changed.
pub fn select_row(state: &mut EditorState, index: usize) -> bool {
    let count = rows(state).len();
    let Some(session) = switcher(state) else {
        return false;
    };
    let before = (session.selected, session.first_visible);
    session.select(index, count);
    before != (session.selected, session.first_visible)
}

/// Scroll the popup's list by whole rows (positive = down).
pub fn scroll(state: &mut EditorState, rows_down: isize) -> bool {
    let count = rows(state).len();
    let Some(session) = switcher(state) else {
        return false;
    };
    let before = session.first_visible;
    session.scroll_rows(rows_down, count);
    session.first_visible != before
}

/// Choose row `index`: record what the host runs for it and close. `false`
/// (the switcher stays open) when there is no such row.
pub fn choose(state: &mut EditorState, index: usize) -> bool {
    if !is_active(state) {
        return false;
    }
    let Some(row) = rows(state).into_iter().nth(index) else {
        return false;
    };
    match row.target {
        QuickSwitcherTarget::Document(id) => {
            state.editor_ui.pending_document_request = Some(DocumentTabRequest::Switch(id));
        }
        QuickSwitcherTarget::File(path) => {
            state.editor_ui.projects_panel.pending_request = Some(ProjectRequest::OpenFile(path));
        }
    }
    close(state);
    true
}

/// Enter: choose the highlighted row. `None` when the switcher is closed,
/// `Some(false)` when it is open but lists nothing.
pub fn confirm(state: &mut EditorState) -> Option<bool> {
    let selected = switcher(state)?.selected;
    Some(choose(state, selected))
}

/// Escape: close without choosing. `true` when it was open.
pub fn cancel(state: &mut EditorState) -> bool {
    close(state)
}

/// The selected part of the query, for Copy / Cut.
pub fn selected_text(state: &EditorState) -> Option<String> {
    let switcher = &state.editor_ui.projects_panel.switcher;
    if !switcher.open {
        return None;
    }
    let (start, end) = switcher.input.highlight_range()?;
    switcher.input.text().get(start..end).map(str::to_owned)
}

#[cfg(test)]
#[path = "quick_switcher_keyboard_tests.rs"]
mod tests;
