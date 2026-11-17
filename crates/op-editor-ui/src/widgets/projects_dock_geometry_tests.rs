//! The Projects dock's claim on the window's right edge: every right-edge
//! computation (canvas region, property panel, floating anchors) must
//! subtract the same dock width, so nothing paints or hit-tests under it.

use super::*;
use op_editor_core::{ChatAnchor, NodeId, ProjectView};

const VIEWPORT_W: f32 = 1_600.0;
const VIEWPORT_H: f32 = 900.0;

const ONE_FRAME: &str = r#"{"version":"1.0.0","children":[
    {"type":"frame","id":"hero","name":"Hero","x":0,"y":0,"width":400,"height":300}
]}"#;

fn project() -> ProjectView {
    ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        ..ProjectView::default()
    }
}

/// A desktop window whose host holds several documents and one project, so
/// the dock is on offer and open.
fn docked() -> EditorState {
    let document = jian_ops_schema::load_str(ONE_FRAME)
        .expect("fixture parses")
        .value;
    let mut state = EditorState::from_document(document);
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.projects_panel.projects = vec![project()];
    state.editor_ui.sidebar_open = true;
    state
}

fn with_selection(mut state: EditorState) -> EditorState {
    state.set_single_selection(NodeId::new("hero"));
    assert!(state.property_panel_visible());
    state
}

fn right(rect: Rect) -> f32 {
    rect.origin.x + rect.size.x
}

#[test]
fn canvas_region_excludes_the_projects_dock() {
    let state = docked();
    let dock = projects_dock_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(dock.size.x, DOCK_WIDTH);
    assert_eq!(dock.origin.x, VIEWPORT_W - DOCK_WIDTH);
    assert_eq!(dock.origin.y, TOP_BAR_HEIGHT);
    assert_eq!(dock.size.y, VIEWPORT_H - TOP_BAR_HEIGHT);

    let canvas = canvas_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(right(canvas), dock.origin.x, "the canvas stops at the dock");

    // With a selection the property panel sits between canvas and dock.
    let state = with_selection(docked());
    let canvas = canvas_rect(&state, VIEWPORT_W, VIEWPORT_H);
    let panel = property_panel_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(right(canvas), panel.origin.x);
    assert_eq!(
        canvas.size.x,
        VIEWPORT_W - DOCK_WIDTH - state.editor_ui.property_panel_width - canvas.origin.x
    );
}

#[test]
fn property_panel_sits_left_of_the_dock() {
    let state = with_selection(docked());
    let dock = projects_dock_rect(&state, VIEWPORT_W, VIEWPORT_H);
    let panel = property_panel_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(right(panel), dock.origin.x, "flush against the dock");
    assert_eq!(panel.size.x, state.editor_ui.property_panel_width);

    // And back at the window edge when the host has no dock at all.
    let mut bare = with_selection(docked());
    bare.editor_ui.document_tabs_supported = false;
    let panel = property_panel_rect(&bare, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(right(panel), VIEWPORT_W);

    // An empty dock is the slim strip: the panel still clears it.
    let mut empty = with_selection(docked());
    empty.editor_ui.projects_panel.projects.clear();
    let panel = property_panel_rect(&empty, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(right(panel), VIEWPORT_W - DOCK_COLLAPSED_WIDTH);
}

#[test]
fn status_bar_and_chat_anchor_clear_the_dock() {
    for state in [docked(), with_selection(docked())] {
        let dock = projects_dock_rect(&state, VIEWPORT_W, VIEWPORT_H);
        let status = status_bar_rect(&state, VIEWPORT_W, VIEWPORT_H).expect("the pill fits");
        assert!(
            right(status) < dock.origin.x,
            "zoom controls clear the dock"
        );

        let (cx0, cy0, cw, ch) = canvas_region(&state, VIEWPORT_W, VIEWPORT_H);
        let bar = minimized_chat_bar_rect(ChatAnchor::BottomRight, 380.0, None, cx0, cy0, cw, ch)
            .expect("the bar fits");
        assert!(right(bar) < dock.origin.x, "the chat card clears the dock");
    }
}

#[test]
fn dock_hidden_when_unsupported_or_touch_and_a_strip_when_empty() {
    let shown = |state: &EditorState| projects_dock_width(state) > 0.0;
    assert!(shown(&docked()));

    let mut state = docked();
    state.editor_ui.document_tabs_supported = false;
    assert!(!shown(&state), "a single-document host has no dock");

    // With no project the dock is still on offer, as the slim strip that
    // holds "Open project".
    let mut state = docked();
    state.editor_ui.projects_panel.projects.clear();
    assert_eq!(projects_dock_width(&state), DOCK_COLLAPSED_WIDTH);
    state.editor_ui.projects_panel.empty_open = true;
    assert_eq!(
        projects_dock_width(&state),
        DOCK_WIDTH,
        "unfolded by choice"
    );

    let mut state = docked();
    state.editor_ui.touch = true;
    assert!(!shown(&state), "touch chrome never docks");

    // Unavailable: a zero-width rect and an undisturbed canvas.
    let mut state = docked();
    state.editor_ui.document_tabs_supported = false;
    assert_eq!(
        projects_dock_rect(&state, VIEWPORT_W, VIEWPORT_H).size.x,
        0.0
    );
    assert_eq!(
        right(canvas_rect(&state, VIEWPORT_W, VIEWPORT_H)),
        VIEWPORT_W
    );
}

#[test]
fn collapsed_dock_is_a_slim_strip_and_canvas_reclaims_the_width() {
    let mut state = docked();
    let open_canvas = canvas_rect(&state, VIEWPORT_W, VIEWPORT_H);
    state.editor_ui.projects_panel.dock_open = false;
    let dock = projects_dock_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(dock.size.x, DOCK_COLLAPSED_WIDTH);
    assert_eq!(right(dock), VIEWPORT_W);
    let canvas = canvas_rect(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(
        canvas.size.x - open_canvas.size.x,
        DOCK_WIDTH - DOCK_COLLAPSED_WIDTH
    );
    assert_eq!(right(canvas), dock.origin.x);
}

#[test]
fn a_point_right_of_the_canvas_is_not_on_the_canvas() {
    let state = with_selection(docked());
    let panel = property_panel_rect(&state, VIEWPORT_W, VIEWPORT_H);
    let y = 400.0;
    // Just left of the property panel is canvas; the panel and the dock
    // beside it, up to the window edge, are not.
    assert!(over_canvas(
        &state,
        panel.origin.x - 1.0,
        y,
        VIEWPORT_W,
        VIEWPORT_H
    ));
    assert!(canvas_doc_point(&state, panel.origin.x - 1.0, y, VIEWPORT_W, VIEWPORT_H).is_some());
    for x in [
        panel.origin.x + 1.0,
        VIEWPORT_W - DOCK_WIDTH + 1.0,
        VIEWPORT_W - 1.0,
    ] {
        assert!(
            !over_canvas(&state, x, y, VIEWPORT_W, VIEWPORT_H),
            "x = {x}"
        );
        assert!(canvas_doc_point(&state, x, y, VIEWPORT_W, VIEWPORT_H).is_none());
    }
}

#[test]
fn the_canvas_centre_is_the_centre_of_the_narrowed_canvas() {
    let state = with_selection(docked());
    let canvas = canvas_rect(&state, VIEWPORT_W, VIEWPORT_H);
    let expected = state
        .viewport
        .to_document(Point2D::new(canvas.size.x / 2.0, canvas.size.y / 2.0));
    let centre = canvas_centre_doc_point(&state, VIEWPORT_W, VIEWPORT_H);
    assert_eq!(centre, expected);
    // The full-width centre would sit under the dock and panel.
    let wide = state
        .viewport
        .to_document(Point2D::new(VIEWPORT_W / 2.0, canvas.size.y / 2.0));
    assert_ne!(centre, wide);
}
