//! Keyboard and mouse input for the document tab strip: the chords and the
//! middle-click must respect text focus, tab count and overlays.

use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabRequest, DocumentTabStatus, DocumentTabView, TopBarButton,
};
use op_editor_ui::widgets::{TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect};

const VIEWPORT_W: f32 = 1400.0;

fn host_with_tabs(count: u64) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.last_viewport_w = VIEWPORT_W;
    host.last_viewport_h = 900.0;
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.home.visible = false;
    ui.document_tabs = (0..count)
        .map(|n| DocumentTabView {
            id: DocumentId(10 + n),
            label: format!("doc-{n}.op"),
            project: String::new(),
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: n == 0,
        })
        .collect();
    host
}

fn pending(host: &WidgetHostNative) -> Option<DocumentTabRequest> {
    host.editor_state().editor_ui.pending_document_request
}

fn clear_pending(host: &mut WidgetHostNative) {
    host.editor_state_mut().editor_ui.pending_document_request = None;
}

/// A point inside tab `index`, clear of its close button.
fn inside_tab(host: &WidgetHostNative, index: usize) -> (f32, f32) {
    let bar = TopBar::for_editor_ui(&host.editor_state().editor_ui);
    let bar_rect = Rect::xywh(0.0, 0.0, VIEWPORT_W, TOP_BAR_HEIGHT);
    let rect = bar
        .button_rect(bar_rect, TopBarButton::DocumentTab(index))
        .expect("the tab is on screen");
    let at = Point2D::new(rect.origin.x + 4.0, rect.origin.y + 3.0);
    (at.x, at.y)
}

#[test]
fn shortcut_ignored_while_canvas_text_edit_focused() {
    let mut host = host_with_tabs(3);
    assert!(host.apply_document_tab_shortcut("2", false));
    assert_eq!(pending(&host), Some(DocumentTabRequest::SwitchToIndex(1)));
    clear_pending(&mut host);

    host.editor_state_mut().ui.text_editing = Some(op_editor_core::NodeId::new("n10"));
    assert!(!host.apply_document_tab_shortcut("2", false), "digit chord");
    assert!(
        !host.apply_document_tab_shortcut("]", true),
        "bracket chord"
    );
    assert!(
        !host.apply_close_document_tab_shortcut(),
        "Cmd+W is left to the text edit"
    );
    assert_eq!(pending(&host), None);
}

#[test]
fn cmd_w_noop_with_single_document() {
    let mut host = host_with_tabs(1);
    assert!(!host.apply_close_document_tab_shortcut());
    assert_eq!(pending(&host), None);

    let mut unsupported = host_with_tabs(3);
    unsupported
        .editor_state_mut()
        .editor_ui
        .document_tabs_supported = false;
    assert!(!unsupported.apply_close_document_tab_shortcut());

    let mut several = host_with_tabs(3);
    assert!(several.apply_close_document_tab_shortcut());
    assert_eq!(
        pending(&several),
        Some(DocumentTabRequest::Close(DocumentId(10)))
    );
}

#[test]
fn middle_click_ignored_under_modal() {
    let mut host = host_with_tabs(2);
    let (x, y) = inside_tab(&host, 1);

    // Nothing covers the strip: the middle-click closes the tab under it.
    assert!(host.apply_pan_press(x, y));
    assert_eq!(
        pending(&host),
        Some(DocumentTabRequest::Close(DocumentId(11)))
    );
    clear_pending(&mut host);

    // A modal over the bar swallows the press; the tab underneath stays.
    host.editor_state_mut().editor_ui.agent_settings_open = true;
    host.apply_pan_press(x, y);
    assert_eq!(pending(&host), None, "settings modal");
    host.editor_state_mut().editor_ui.agent_settings_open = false;

    host.editor_state_mut().editor_ui.file_menu_open = true;
    host.apply_pan_press(x, y);
    assert_eq!(pending(&host), None, "top-bar dropdown");
    host.editor_state_mut().editor_ui.file_menu_open = false;

    host.apply_pan_press(x, y);
    assert_eq!(
        pending(&host),
        Some(DocumentTabRequest::Close(DocumentId(11))),
        "closing the overlay re-enables it"
    );
}
