//! What a press on the document tab strip asks of the host.
//!
//! The strip never touches documents itself: it records a
//! [`DocumentTabRequest`] on `EditorUiState` and the host that owns the
//! document registry drains it. Both hosts reach this function through
//! the shared TopBar press flow, so a tab press is single-sourced. A
//! middle-click resolves its tab with `TopBar::document_tab_index_at` and
//! then asks for the same close as the x button.

use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::DocumentTabRequest;

use crate::widgets::TopBarHit;

/// Record the request a tab hit stands for. Pressing the already-active tab
/// asks for nothing; a stale index (the rows changed under the cursor) is
/// ignored rather than hitting another document.
pub fn apply_document_tab_hit(ui: &mut EditorUiState, hit: TopBarHit) {
    let request = match hit {
        TopBarHit::DocumentTab(index) => ui
            .document_tabs
            .get(index)
            .filter(|tab| !tab.active)
            .map(|tab| DocumentTabRequest::Switch(tab.id)),
        TopBarHit::CloseDocumentTab(index) => ui
            .document_tabs
            .get(index)
            .map(|tab| DocumentTabRequest::Close(tab.id)),
        _ => None,
    };
    if request.is_some() {
        ui.pending_document_request = request;
    }
}
