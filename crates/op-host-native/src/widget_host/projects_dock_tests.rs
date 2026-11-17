//! Native-host routing for the Projects dock: a click, hover or wheel on the
//! window's far-right column belongs to the dock and never reaches the
//! canvas, the property panel's gutter still tracks its shifted edge, and a
//! drag that wanders onto the dock keeps following the pointer.
//!
//! Windows-gated like the other tests that solve layout: building the scene
//! runs `jian_skia::SkiaMeasure`, which aborts under Windows CI's DirectWrite.

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::{PanelResizeKind, WidgetHostNative};
use op_editor_core::{EditorState, NodeId, ProjectFileView, ProjectRequest, ProjectView};
use op_editor_ui::widgets::host_canvas_geometry as geometry;
use op_editor_ui::widgets::projects_dock_layout::DOCK_WIDTH;
use op_editor_ui::Point2D;

const VW: f32 = 1_400.0;
const VH: f32 = 900.0;

/// One board far wider than the window, so it runs under the dock, the
/// property panel and the canvas alike when the viewport is at identity.
const WIDE_BOARD: &str = r##"{
    "version": "1.0.0",
    "children": [
        { "type": "frame", "id": "board", "name": "Board", "x": 0, "y": 0,
          "width": 4000, "height": 500,
          "fill": [{"type":"solid","color":"#ffffff"}], "children": [] }
    ]
}"##;

fn project() -> ProjectView {
    ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![ProjectFileView {
            path: "/work/shop/designs/cart.op".to_string(),
            label: "cart.op".to_string(),
            open_id: None,
            dirty: false,
            status: op_editor_core::DocumentTabStatus::Idle,
            active: false,
            folder: "designs".to_string(),
            kind: op_editor_core::ProjectFileKind::Design,
            missing: false,
            removable: false,
        }],
        ..ProjectView::default()
    }
}

fn host() -> WidgetHostNative {
    let document = jian_ops_schema::load_str(WIDE_BOARD)
        .expect("parse fixture")
        .value;
    let mut host = WidgetHostNative::new();
    host.install_imported_state(EditorState::from_document(document));
    let ui = &mut host.editor_state.editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = vec![project()];
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    host.editor_state.viewport = op_editor_core::Viewport::IDENTITY;
    host.mark_dirty();
    host
}

fn dock(host: &WidgetHostNative) -> op_editor_ui::Rect {
    geometry::projects_dock_rect(&host.editor_state, VW, VH)
}

/// A point on the dock's bare lower surface: no row, no header.
fn bare_dock_point(host: &WidgetHostNative) -> Point2D {
    let rect = dock(host);
    Point2D::new(rect.origin.x + 40.0, rect.origin.y + rect.size.y - 20.0)
}

#[test]
fn click_in_dock_never_reaches_the_canvas() {
    let mut host = host();
    // The board spans the whole window, so the same click without the dock
    // would select it.
    let canvas = geometry::canvas_rect(&host.editor_state, VW, VH);
    let on_canvas = Point2D::new(canvas.origin.x + 200.0, canvas.origin.y + 100.0);
    let in_dock = bare_dock_point(&host);
    assert!(host.editor_state.selection.set.is_empty());

    assert!(
        host.apply_press(in_dock.x, in_dock.y, VW, VH),
        "the dock consumes the press"
    );
    host.apply_release_with_viewport(VW, VH);
    assert!(
        host.editor_state.selection.set.is_empty(),
        "a press on the dock must not select the board under it"
    );
    assert!(host.marquee_drag.is_none(), "nor start a marquee");

    // A selected board stays selected: the dock's press is not a blank
    // canvas press, which would clear the selection.
    host.editor_state.set_single_selection(NodeId::new("board"));
    host.mark_dirty();
    let in_dock = bare_dock_point(&host);
    assert!(host.apply_press(in_dock.x, in_dock.y, VW, VH));
    host.apply_release_with_viewport(VW, VH);
    assert!(host
        .editor_state
        .selection
        .set
        .contains(&NodeId::new("board")));

    host.editor_state.selection.set.clear();
    host.mark_dirty();
    host.apply_press(on_canvas.x, on_canvas.y, VW, VH);
    host.apply_release_with_viewport(VW, VH);
    assert!(
        host.editor_state
            .selection
            .set
            .contains(&NodeId::new("board")),
        "the same board is selectable on the canvas, left of the dock"
    );
}

#[test]
fn a_node_at_the_right_edge_hit_tests_to_the_canvas_only_left_of_the_dock_and_panel() {
    let mut host = host();
    host.editor_state.set_single_selection(NodeId::new("board"));
    host.mark_dirty();
    let panel = host.property_rect(VW, VH);
    let y = 120.0;
    // The board spans every x, so the canvas test alone decides.
    assert!(host.over_canvas(panel.origin.x - 2.0, y, VW, VH));
    for x in [panel.origin.x + 2.0, VW - DOCK_WIDTH + 2.0, VW - 2.0] {
        assert!(!host.over_canvas(x, y, VW, VH), "x = {x} is not canvas");
    }
    assert_eq!(
        panel.origin.x + panel.size.x,
        VW - DOCK_WIDTH,
        "the property panel is flush against the dock"
    );
}

#[test]
fn clicking_a_file_in_the_dock_records_the_open_request() {
    let mut host = host();
    let rect = dock(&host);
    let layout = op_editor_ui::widgets::projects_dock_flow::layout(&host.editor_state, VW, VH)
        .expect("dock shown");
    let row = layout
        .list
        .as_ref()
        .and_then(|list| {
            list.row(op_editor_ui::widgets::projects_panel_layout::RowKind::File(
                0, 0,
            ))
        })
        .expect("file row")
        .rect;
    let at = Point2D::new(row.origin.x + 60.0, row.origin.y + row.size.y / 2.0);
    assert!(rect.contains(at));
    host.apply_cursor_move(at.x, at.y);
    assert!(host.apply_press(at.x, at.y, VW, VH));
    assert_eq!(
        host.editor_state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/designs/cart.op".into()
        ))
    );
}

#[test]
fn wheel_over_the_dock_never_zooms_the_canvas() {
    let mut host = host();
    let zoom = host.editor_state.viewport.zoom;
    let at = bare_dock_point(&host);
    assert!(host.apply_wheel(at.x, at.y, -120.0, VW, VH));
    assert_eq!(host.editor_state.viewport.zoom, zoom);
    // The same wheel on the canvas does zoom, so the check above is real.
    let canvas = geometry::canvas_rect(&host.editor_state, VW, VH);
    host.apply_wheel(
        canvas.origin.x + 100.0,
        canvas.origin.y + 100.0,
        -120.0,
        VW,
        VH,
    );
    assert_ne!(host.editor_state.viewport.zoom, zoom);
}

#[test]
fn a_marquee_dragged_onto_the_dock_keeps_following_the_pointer() {
    let mut host = host();
    // Empty canvas below the 500 px board.
    let canvas = geometry::canvas_rect(&host.editor_state, VW, VH);
    let start = Point2D::new(canvas.origin.x + 100.0, 760.0);
    host.apply_press(start.x, start.y, VW, VH);
    assert!(
        host.marquee_drag.is_some(),
        "an empty-canvas press starts a marquee"
    );
    let in_dock = bare_dock_point(&host);
    host.apply_cursor_move(in_dock.x, in_dock.y);
    let marquee = host.marquee_drag.expect("marquee still live");
    assert_eq!(marquee.current_screen_x, in_dock.x);
    assert_eq!(marquee.current_screen_y, in_dock.y);
    host.apply_release_with_viewport(VW, VH);
}

#[test]
fn the_property_resize_gutter_tracks_its_edge_beside_the_dock() {
    let mut host = host();
    host.editor_state.set_single_selection(NodeId::new("board"));
    host.mark_dirty();
    let panel = host.property_rect(VW, VH);
    let y = 300.0;
    assert_eq!(
        host.panel_resize_hover(panel.origin.x, y, VW),
        Some(PanelResizeKind::PropertyLeft)
    );
    // The window-edge position the gutter used to sit at is now the dock.
    let old_edge = VW - host.editor_state.editor_ui.property_panel_width;
    assert_eq!(host.panel_resize_hover(old_edge, y, VW), None);

    // Dragging the gutter 30 px left widens the panel by 30 px.
    let before = host.editor_state.editor_ui.property_panel_width;
    assert!(host.apply_press(panel.origin.x, y, VW, VH));
    host.apply_cursor_move(panel.origin.x - 30.0, y);
    host.apply_release_with_viewport(VW, VH);
    assert_eq!(
        host.editor_state.editor_ui.property_panel_width,
        before + 30.0
    );
}

#[test]
fn collapsing_the_dock_hands_its_width_back_to_the_canvas() {
    let mut host = host();
    let open = geometry::canvas_rect(&host.editor_state, VW, VH);
    let chevron = {
        let layout =
            op_editor_ui::widgets::projects_dock_flow::layout(&host.editor_state, VW, VH).unwrap();
        Point2D::new(
            layout.toggle.origin.x + layout.toggle.size.x / 2.0,
            layout.toggle.origin.y + layout.toggle.size.y / 2.0,
        )
    };
    assert!(host.apply_press(chevron.x, chevron.y, VW, VH));
    assert!(!host.editor_state.editor_ui.projects_panel.dock_open);
    let collapsed = geometry::canvas_rect(&host.editor_state, VW, VH);
    assert!(collapsed.size.x > open.size.x);
    // The strip is still a surface: a click on it re-opens the dock.
    let strip = dock(&host);
    assert!(host.apply_press(strip.origin.x + 4.0, strip.origin.y + 200.0, VW, VH));
    assert!(host.editor_state.editor_ui.projects_panel.dock_open);
}

#[test]
fn the_chat_card_and_zoom_controls_stay_left_of_the_dock() {
    let mut host = host();
    for width in [1_000.0_f32, 1_400.0] {
        host.last_viewport_w = width;
        let dock_left = geometry::projects_dock_rect(&host.editor_state, width, VH)
            .origin
            .x;
        let chat = host
            .ai_chat_rect(width, VH)
            .expect("the composer card fits the narrowed canvas");
        assert!(
            chat.origin.x + chat.size.x <= dock_left,
            "{width}: chat card ends at {}, dock starts at {dock_left}",
            chat.origin.x + chat.size.x
        );
        let status = host
            .status_bar_rect(width, VH)
            .expect("the zoom pill fits the narrowed canvas");
        assert!(status.origin.x + status.size.x <= dock_left);
    }
    // The expanded floating panel and its minimized bar are placed in the
    // same canvas region, at every anchor.
    for anchor in [
        op_editor_core::ChatAnchor::TopRight,
        op_editor_core::ChatAnchor::BottomRight,
    ] {
        let (cx0, cy0, cw, ch) = host.canvas_region(VW, VH);
        let bar = geometry::minimized_chat_bar_rect(anchor, 380.0, None, cx0, cy0, cw, ch)
            .expect("the bar fits");
        assert!(bar.origin.x + bar.size.x <= dock(&host).origin.x);
    }
}

#[test]
fn empty_dock_is_a_collapsed_strip_that_opens_the_picker() {
    let mut host = host();
    host.editor_state.editor_ui.projects_panel.projects.clear();
    host.mark_dirty();
    let strip = dock(&host);
    assert_eq!(
        strip.size.x,
        op_editor_ui::widgets::projects_dock_layout::DOCK_COLLAPSED_WIDTH,
        "no project: a slim strip, not the open column"
    );
    let canvas = geometry::canvas_rect(&host.editor_state, VW, VH);
    assert_eq!(
        canvas.origin.x + canvas.size.x,
        strip.origin.x,
        "the canvas stops short of the strip like it does of the column"
    );

    let layout = host
        .projects_dock_layout(VW, VH)
        .expect("the empty dock is laid out");
    let button = layout.open_project.expect("the strip offers Open project");
    let at = Point2D::new(button.origin.x + 4.0, button.origin.y + 4.0);
    assert!(
        host.apply_press(at.x, at.y, VW, VH),
        "the strip owns the press"
    );
    host.apply_release_with_viewport(VW, VH);
    assert_eq!(
        host.editor_state.editor_ui.projects_panel.pending_request,
        Some(op_editor_core::ProjectRequest::OpenProjectPicker),
        "the host's pump will show the picker"
    );
    assert!(
        !host.editor_state.editor_ui.projects_panel.is_open(),
        "asking for the picker does not unfold the strip"
    );
}
