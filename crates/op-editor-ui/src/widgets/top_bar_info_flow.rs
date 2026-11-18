//! What a press on the info bar's chips asks of the editor.
//!
//! Shared by both hosts through `press_flow::apply_shared_top_bar_hit`, so a
//! chip press is single-sourced. The chips never touch documents themselves:
//! they record a request (`DocumentTabRequest`, `ProjectRequest`) or open a
//! shared surface, and the host that owns the registry executes it.

use op_editor_core::{ActiveFileInfo, DocumentTabRequest, EditorState};

use op_editor_core::host_press_transitions::close_top_bar_dropdowns;

/// The "N other unsaved" chip: open the quick switcher on the first other
/// unsaved document. It is the one place to find them without the dock.
pub fn open_switcher_on_other_unsaved(state: &mut EditorState, now_ms: u64) {
    // The switcher floats over every dropdown; leave none open under it.
    close_top_bar_dropdowns(&mut state.editor_ui);
    state.chat.blur_input(now_ms);
    op_editor_core::quick_switcher_keyboard::open_on_other_unsaved(state, now_ms);
}

/// The breadcrumb: ask the dock to reveal the active file's row. Reveal only:
/// the rename stays in the dock. A file the dock does not list (the chip is
/// then not clickable) asks for nothing.
pub fn reveal_active_file(state: &mut EditorState) {
    let info = ActiveFileInfo::from_ui(&state.editor_ui, "");
    if let Some(path) = info.file_path {
        state.editor_ui.projects_panel.pending_request =
            Some(op_editor_core::ProjectRequest::RevealFile(path));
    }
}

/// The agent-activity item: switch to the file the agent is running in.
/// A stale chip (the run ended since the last frame) asks for nothing.
pub fn switch_to_running_document(state: &mut EditorState) {
    let info = ActiveFileInfo::from_ui(&state.editor_ui, "");
    if let Some((id, _)) = info.running_elsewhere.first {
        state.editor_ui.pending_document_request = Some(DocumentTabRequest::Switch(id));
    }
}

#[cfg(test)]
#[path = "top_bar_info_flow_tests.rs"]
mod tests;
