//! Native-host routing of the Cmd+K quick switcher: it opens only when nothing
//! else owns the keyboard, swallows every pointer event while open (a press on
//! the canvas, the wheel, a right or middle press), and takes typed text. Also
//! the dock's middle-click close.
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabRequest, DocumentTabStatus, DocumentTabView, EditorState, NodeId,
    ProjectFileView, ProjectRequest, ProjectView,
};
use op_editor_ui::widgets::host_canvas_geometry as geometry;
use op_editor_ui::widgets::projects_dock_flow as dock_flow;
use op_editor_ui::widgets::projects_panel_layout::RowKind;
use op_editor_ui::widgets::quick_switcher_flow;
use op_editor_ui::Point2D;

const VW: f32 = 1_400.0;
const VH: f32 = 900.0;

const WIDE_BOARD: &str = r##"{
    "version": "1.0.0",
    "children": [
        { "type": "frame", "id": "board", "name": "Board", "x": 0, "y": 0,
          "width": 4000, "height": 500,
          "fill": [{"type":"solid","color":"#ffffff"}], "children": [] }
    ]
}"##;

fn tab(id: u64, label: &str, active: bool) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: "shop".to_string(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active,
    }
}

fn file(label: &str, open: Option<u64>) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: open == Some(1),
        folder: String::new(),
        kind: op_editor_core::ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

/// Two open documents (`home.op`, `cart.op`) and one closed file.
fn host() -> WidgetHostNative {
    let document = jian_ops_schema::load_str(WIDE_BOARD)
        .expect("parse fixture")
        .value;
    let mut host = WidgetHostNative::new();
    host.install_imported_state(EditorState::from_document(document));
    let ui = &mut host.editor_state.editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.document_tabs = vec![tab(1, "home.op", true), tab(2, "cart.op", false)];
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![
            file("home.op", Some(1)),
            file("cart.op", Some(2)),
            file("pricing.op", None),
        ],
        ..ProjectView::default()
    }];
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    host.editor_state.viewport = op_editor_core::Viewport::IDENTITY;
    host.mark_dirty();
    host
}

fn on_canvas(host: &WidgetHostNative) -> Point2D {
    let canvas = geometry::canvas_rect(&host.editor_state, VW, VH);
    Point2D::new(canvas.origin.x + 200.0, canvas.origin.y + 300.0)
}

#[test]
fn the_chord_needs_no_focused_text_field_and_no_other_modal() {
    let mut host = host();
    host.editor_state.chat.focused = true;
    assert!(!host.apply_toggle_quick_switcher(), "chat input has focus");
    host.editor_state.chat.focused = false;
    assert!(host.apply_toggle_quick_switcher());
    assert!(host.quick_switcher_active());
    assert!(
        host.apply_toggle_quick_switcher(),
        "the chord closes it again"
    );
    assert!(!host.quick_switcher_active());
    host.editor_state.editor_ui.file_menu_open = true;
    assert!(!host.apply_toggle_quick_switcher(), "a menu is open");
    host.editor_state.editor_ui.file_menu_open = false;
    host.editor_state.editor_ui.document_tabs_supported = false;
    assert!(!host.apply_toggle_quick_switcher(), "no dock on this host");
}

#[test]
fn an_open_switcher_swallows_a_canvas_press_and_does_not_select() {
    let mut host = host();
    let at = on_canvas(&host);
    // Without the switcher the same press selects the board (the check is real).
    host.apply_press(at.x, at.y, VW, VH);
    host.apply_release_with_viewport(VW, VH);
    assert!(host
        .editor_state
        .selection
        .set
        .contains(&NodeId::new("board")));
    host.editor_state.selection.set.clear();
    host.mark_dirty();

    assert!(host.apply_toggle_quick_switcher());
    assert!(host.apply_press(at.x, at.y, VW, VH), "claimed");
    host.apply_release_with_viewport(VW, VH);
    assert!(
        host.editor_state.selection.set.is_empty(),
        "nothing under it reacted"
    );
    assert!(host.marquee_drag.is_none());
    assert!(!host.quick_switcher_active(), "a press outside closes it");
}

#[test]
fn an_open_switcher_swallows_the_wheel_right_and_middle_presses() {
    let mut host = host();
    let at = on_canvas(&host);
    host.apply_toggle_quick_switcher();
    let zoom = host.editor_state.viewport.zoom;
    assert!(host.apply_wheel(at.x, at.y, -120.0, VW, VH));
    assert_eq!(
        host.editor_state.viewport.zoom, zoom,
        "the canvas does not zoom"
    );
    assert!(host.apply_right_press(at.x, at.y, VW, VH));
    assert!(host.editor_state.editor_ui.layer_context_menu.is_none());
    assert!(
        host.apply_pan_press(at.x, at.y),
        "middle press is swallowed"
    );
    assert!(host.drag.is_none(), "no canvas pan starts");
    assert!(host.quick_switcher_active(), "none of them closed it");
}

#[test]
fn the_pointer_over_a_row_highlights_it_and_a_click_chooses_it() {
    let mut host = host();
    host.apply_toggle_quick_switcher();
    let layout = quick_switcher_flow::layout(&host.editor_state, VW).expect("open");
    let (index, rect) = layout.rows[2];
    let at = Point2D::new(rect.origin.x + 30.0, rect.origin.y + rect.size.y / 2.0);
    host.apply_cursor_move(at.x, at.y);
    assert_eq!(
        host.editor_state.editor_ui.projects_panel.switcher.selected,
        index
    );
    assert!(host.apply_press(at.x, at.y, VW, VH));
    assert_eq!(
        host.editor_state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile("/work/shop/pricing.op".into()))
    );
    assert!(!host.quick_switcher_active());
}

#[test]
fn typed_text_reaches_the_query_and_enter_escape_and_backspace_work() {
    let mut host = host();
    host.apply_toggle_quick_switcher();
    for c in "cart".chars() {
        assert!(host.apply_text(c));
    }
    assert_eq!(
        host.editor_state
            .editor_ui
            .projects_panel
            .switcher
            .input
            .text(),
        "cart"
    );
    assert!(
        host.input_active_pub(),
        "tool shortcuts see an input owning the keyboard"
    );
    assert!(host.apply_quick_switcher_backspace());
    assert!(host.apply_text('t'));
    assert!(host.apply_quick_switcher_confirm());
    assert_eq!(
        host.editor_state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(2)))
    );
    assert!(!host.quick_switcher_active());
    // Escape closes without choosing.
    host.editor_state.editor_ui.pending_document_request = None;
    host.apply_toggle_quick_switcher();
    assert!(host.apply_quick_switcher_cancel());
    assert_eq!(host.editor_state.editor_ui.pending_document_request, None);
}

#[test]
fn the_card_paints_above_the_chrome_only_while_open() {
    let mut host = host();
    let paint = |host: &mut WidgetHostNative| {
        let mut backend = crate::backend::NativeBackend::with_dpi(1.0);
        let mut surface = skia_safe::surfaces::raster_n32_premul((VW as i32, VH as i32)).unwrap();
        let mut frame = crate::backend::NativeFrameBackend::new(&mut backend, surface.canvas());
        host.paint(&mut frame, VW, VH);
        let layout = quick_switcher_flow::layout(&host.editor_state, VW);
        let probe = layout
            .map(|l| (l.card.origin.x as i32 + 4, l.card.origin.y as i32 + 4))
            .unwrap_or((VW as i32 / 2, 60));
        let c = surface.peek_pixels().unwrap().get_color(probe);
        [c.r(), c.g(), c.b()]
    };
    let closed = paint(&mut host);
    host.apply_toggle_quick_switcher();
    let open = paint(&mut host);
    assert_ne!(
        closed, open,
        "the raised card replaces what was at that pixel"
    );
}

#[test]
fn a_middle_press_on_an_open_files_row_closes_it_and_a_closed_row_is_swallowed() {
    let mut host = host();
    let layout = dock_flow::layout(&host.editor_state, VW, VH).expect("dock");
    let list = layout.list.as_ref().expect("list");
    let point = |kind: RowKind| {
        let rect = list.row(kind).expect("row").rect;
        Point2D::new(rect.origin.x + 60.0, rect.origin.y + rect.size.y / 2.0)
    };
    let open_row = point(RowKind::File(0, 1));
    assert!(host.apply_pan_press(open_row.x, open_row.y));
    assert_eq!(
        host.editor_state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(2)))
    );
    assert!(host.drag.is_none(), "no pan under the dock");

    host.editor_state.editor_ui.pending_document_request = None;
    let closed_row = point(RowKind::File(0, 2));
    assert!(host.apply_pan_press(closed_row.x, closed_row.y));
    assert_eq!(host.editor_state.editor_ui.pending_document_request, None);
    assert!(host.drag.is_none());

    // Off the dock a middle press still starts the canvas pan.
    let at = on_canvas(&host);
    host.apply_pan_press(at.x, at.y);
    assert!(host.drag.is_some());
}
