use super::top_bar_document_tabs_flow::*;
use crate::widgets::press_flow::{apply_shared_top_bar_hit, TopBarPress};
use crate::widgets::{TopBar, TopBarHit, TOP_BAR_HEIGHT};
use crate::{Point2D, Rect};
use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::{
    DocumentId, DocumentTabRequest, DocumentTabStatus, DocumentTabView, EditorState,
};

fn width_of(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.6
}

fn state_with_tabs() -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.document_tabs = (0..3)
        .map(|n| DocumentTabView {
            id: DocumentId(10 + n),
            label: format!("doc-{n}.op"),
            project: String::new(),
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: n == 0,
        })
        .collect();
    state
}

#[test]
fn press_on_tab_requests_switch() {
    let mut state = state_with_tabs();
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::DocumentTab(2), 0);
    assert_eq!(outcome, TopBarPress::Handled);
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(12)))
    );
}

#[test]
fn press_on_active_tab_requests_nothing() {
    let mut state = state_with_tabs();
    apply_shared_top_bar_hit(&mut state, TopBarHit::DocumentTab(0), 0);
    assert_eq!(state.editor_ui.pending_document_request, None);
}

#[test]
fn press_on_close_requests_close() {
    let mut state = state_with_tabs();
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::CloseDocumentTab(1), 0);
    assert_eq!(outcome, TopBarPress::Handled);
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(11)))
    );
}

#[test]
fn stale_tab_index_is_ignored() {
    let mut state = state_with_tabs();
    apply_shared_top_bar_hit(&mut state, TopBarHit::DocumentTab(7), 0);
    apply_shared_top_bar_hit(&mut state, TopBarHit::CloseDocumentTab(7), 0);
    assert_eq!(state.editor_ui.pending_document_request, None);
}

/// What a host's middle-click does: resolve the tab under the cursor, then
/// ask for the same close as the x button.
fn middle_click(ui: &mut EditorUiState, point: Point2D) -> bool {
    let rect = Rect::xywh(0.0, 0.0, 1200.0, TOP_BAR_HEIGHT);
    let Some(index) = TopBar::for_editor_ui(ui).document_tab_index_at(rect, point, width_of) else {
        return false;
    };
    apply_document_tab_hit(ui, TopBarHit::CloseDocumentTab(index));
    true
}

#[test]
fn middle_click_closes() {
    let mut ui: EditorUiState = state_with_tabs().editor_ui;
    let rect = Rect::xywh(0.0, 0.0, 1200.0, TOP_BAR_HEIGHT);
    assert!(!middle_click(&mut ui, Point2D::new(0.0, 0.0)));
    assert_eq!(ui.pending_document_request, None);

    let anchor = TopBar::for_editor_ui(&ui)
        .button_rect_with_measure(rect, op_editor_core::TopBarButton::DocumentTab(1), width_of)
        .expect("second tab is laid out");
    let on_tab = Point2D::new(anchor.origin.x + 12.0, anchor.origin.y + 12.0);
    assert!(middle_click(&mut ui, on_tab));
    assert_eq!(
        ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(11)))
    );
}

#[test]
fn middle_click_does_nothing_when_the_strip_is_hidden() {
    let mut ui = state_with_tabs().editor_ui;
    ui.document_tabs_supported = false;
    for x in (0..1200).step_by(20) {
        assert!(!middle_click(&mut ui, Point2D::new(x as f32, 20.0)));
    }
    assert_eq!(ui.pending_document_request, None);
}
