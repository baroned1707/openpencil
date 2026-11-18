//! Cmd/Ctrl chords that move between open documents.
//!
//! The host decides that the platform command modifier is held and whether
//! a text input owns the keyboard; this module only maps the key to the
//! request the document registry will execute. Everything is a no-op unless
//! the host registered support for several documents, so the web build and
//! single-document desktop never see these chords.

use crate::document_tabs::DocumentTabRequest;
use crate::editor_ui_state::EditorUiState;

/// The request for Cmd/Ctrl+`key` (`shift` = Shift held), or `None` when the
/// chord is not a tab chord or must be left to something else.
///
/// - `1`..`8` switch to that position (`SwitchToIndex(n - 1)`) when a tab
///   exists there; `9` switches to the last tab.
/// - Shift+`[` / Shift+`]` select the previous / next tab and wrap around.
///   Shift changes the character winit reports, so `{` / `}` count as well.
///
/// `text_input_focused` is the host's "a text surface owns the keyboard"
/// predicate; the chord never fires while one does.
pub fn document_tab_shortcut(
    ui: &EditorUiState,
    key: &str,
    shift: bool,
    text_input_focused: bool,
) -> Option<DocumentTabRequest> {
    if !ui.document_tabs_supported || text_input_focused || ui.document_tabs.len() < 2 {
        return None;
    }
    let step: isize = match (key, shift) {
        ("[" | "{", true) => -1,
        ("]" | "}", true) => 1,
        (digit, false) => {
            let number = digit.chars().next().and_then(|c| c.to_digit(10))?;
            if digit.chars().count() != 1 || number == 0 {
                return None;
            }
            // `9` is "the last tab", as in browsers, however many are open.
            let index = if number == 9 {
                ui.document_tabs.len() - 1
            } else {
                number as usize - 1
            };
            // A position past the last tab is not a request worth raising.
            return (index < ui.document_tabs.len())
                .then_some(DocumentTabRequest::SwitchToIndex(index));
        }
        _ => return None,
    };
    let count = ui.document_tabs.len() as isize;
    let active = ui.document_tabs.iter().position(|tab| tab.active)? as isize;
    let target = (active + step).rem_euclid(count) as usize;
    Some(DocumentTabRequest::Switch(ui.document_tabs[target].id))
}

/// The digit of the Cmd/Ctrl+digit chord that reaches the tab at `index` of
/// `tab_count` open tabs, or `None` when no digit does (the chord also does
/// nothing at all below two tabs). The dock paints this as the open mark of a
/// file, so it mirrors [`document_tab_shortcut`] exactly: `1`..`8` reach
/// positions 1..8, `9` reaches the LAST tab however many are open, and a tab
/// in between (position 9 and up, but not the last) has no chord.
pub fn shortcut_digit_for(tab_count: usize, index: usize) -> Option<u8> {
    if tab_count < 2 || index >= tab_count {
        return None;
    }
    match index {
        0..=7 => Some(index as u8 + 1),
        _ if index == tab_count - 1 => Some(9),
        _ => None,
    }
}

/// The request for Cmd/Ctrl+W: close the document on screen. `None` unless the
/// host has tabs, more than one is open, and no text input owns the keyboard —
/// in every other case the chord keeps its ordinary meaning.
pub fn close_document_tab_shortcut(
    ui: &EditorUiState,
    text_input_focused: bool,
) -> Option<DocumentTabRequest> {
    if !ui.document_tabs_supported || text_input_focused || ui.document_tabs.len() < 2 {
        return None;
    }
    ui.document_tabs
        .iter()
        .find(|tab| tab.active)
        .map(|tab| DocumentTabRequest::Close(tab.id))
}

#[cfg(test)]
#[path = "document_tab_shortcuts_tests.rs"]
mod tests;
