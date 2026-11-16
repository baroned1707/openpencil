//! Flow tests for the Projects dock: what a click on its chevron, a file or
//! its bare surface does, hover, and the wheel.

use super::*;
use crate::widgets::projects_dock_layout::{DOCK_COLLAPSED_WIDTH, DOCK_HEADER_HEIGHT};
use crate::widgets::projects_panel_flow::projects_panel_flow_tests::{file, project, shop};
use crate::widgets::projects_panel_layout::RowKind;
use crate::Rect;
use op_editor_core::{ProjectRequest, ProjectRowTarget, ProjectView};

const VIEWPORT_W: f32 = 1_400.0;
const VIEWPORT_H: f32 = 800.0;

fn state_with(projects: Vec<ProjectView>) -> EditorState {
    let mut state = EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.projects_panel.projects = projects;
    state
}

fn dock(state: &EditorState) -> ProjectsDockLayout {
    layout(state, VIEWPORT_W, VIEWPORT_H).expect("the dock is shown")
}

fn row_point(layout: &ProjectsDockLayout, kind: RowKind) -> Point2D {
    let rect = layout
        .list
        .as_ref()
        .and_then(|list| list.row(kind))
        .expect("row is laid out")
        .rect;
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0 - 30.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn dock_click_on_file_requests_switch_or_open() {
    let mut state = state_with(shop());
    let layout = dock(&state);
    // `checkout.op` is open in document 7; `cart.op` is only on disk.
    let open_file = row_point(&layout, RowKind::File(0, 0));
    cursor_move(&mut state, &layout, open_file);
    assert_eq!(press(&mut state, &layout, open_file), DockPress::Claimed);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::SwitchTo(op_editor_core::DocumentId(7)))
    );

    let disk_file = row_point(&layout, RowKind::File(0, 1));
    cursor_move(&mut state, &layout, disk_file);
    assert_eq!(press(&mut state, &layout, disk_file), DockPress::Claimed);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/designs/cart.op".into()
        ))
    );
}

#[test]
fn collapse_chevron_toggles_dock_open() {
    let mut state = state_with(shop());
    assert!(state.editor_ui.projects_panel.dock_open, "open by default");
    let layout = dock(&state);
    let chevron = center(layout.toggle);
    assert_eq!(layout.hit(chevron), Some(DockHit::Toggle));
    assert_eq!(press(&mut state, &layout, chevron), DockPress::Claimed);
    assert!(
        !state.editor_ui.projects_panel.dock_open,
        "chevron folds it"
    );

    // Collapsed, the whole slim strip re-opens it.
    let strip = dock(&state);
    assert_eq!(strip.rect.size.x, DOCK_COLLAPSED_WIDTH);
    assert!(strip.list.is_none());
    let anywhere = Point2D::new(
        strip.rect.origin.x + 3.0,
        strip.rect.origin.y + strip.rect.size.y - 40.0,
    );
    assert_eq!(press(&mut state, &strip, anywhere), DockPress::Claimed);
    assert!(
        state.editor_ui.projects_panel.dock_open,
        "strip re-opens it"
    );
}

#[test]
fn a_press_on_the_header_or_bare_dock_is_swallowed_without_a_request() {
    let mut state = state_with(shop());
    let layout = dock(&state);
    let header = Point2D::new(layout.rect.origin.x + 20.0, layout.rect.origin.y + 4.0);
    assert_eq!(layout.hit(header), Some(DockHit::Header));
    assert_eq!(press(&mut state, &layout, header), DockPress::Claimed);
    let bare = Point2D::new(
        layout.rect.origin.x + 20.0,
        layout.rect.origin.y + layout.rect.size.y - 8.0,
    );
    assert_eq!(press(&mut state, &layout, bare), DockPress::Claimed);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
    assert!(state.editor_ui.projects_panel.dock_open);

    let off = Point2D::new(layout.rect.origin.x - 1.0, 300.0);
    assert_eq!(press(&mut state, &layout, off), DockPress::Missed);
}

fn many_projects() -> Vec<ProjectView> {
    (0..12)
        .map(|n| {
            project(
                &format!("project-{n}"),
                (0..4).map(|f| file(&format!("f{f}.op"), None)).collect(),
                true,
            )
        })
        .collect()
}

#[test]
fn wheel_scrolls_the_dock_list() {
    let mut state = state_with(many_projects());
    let layout = dock(&state);
    let inside = Point2D::new(
        layout.rect.origin.x + 40.0,
        layout.rect.origin.y + DOCK_HEADER_HEIGHT + 60.0,
    );
    let max = layout.list.as_ref().unwrap().max_scroll();
    assert!(max > 0.0, "twelve projects overflow the dock");
    assert_eq!(
        scroll(&mut state, Some(&layout), inside, -100_000.0),
        Some(true)
    );
    assert_eq!(state.editor_ui.projects_panel.scroll.offset, max);

    // A wheel over the header is the dock's too (no canvas zoom through it),
    // but moves nothing; a wheel off the dock keeps looking.
    let layout = dock(&state);
    let header = Point2D::new(layout.rect.origin.x + 20.0, layout.rect.origin.y + 4.0);
    assert_eq!(
        scroll(&mut state, Some(&layout), header, -50.0),
        Some(false)
    );
    let off = Point2D::new(layout.rect.origin.x - 5.0, inside.y);
    assert_eq!(scroll(&mut state, Some(&layout), off, -50.0), None);
    assert_eq!(scroll(&mut state, None, inside, -50.0), None);
}

#[test]
fn hover_tracks_rows_and_the_chevron_and_clears() {
    let mut state = state_with(shop());
    let layout = dock(&state);
    let file_at = row_point(&layout, RowKind::File(0, 1));
    assert!(cursor_move(&mut state, &layout, file_at));
    assert_eq!(
        state.editor_ui.projects_panel.hover,
        Some(ProjectRowTarget::File(0, 1))
    );
    assert!(
        !cursor_move(&mut state, &layout, file_at),
        "no change, no repaint"
    );

    assert!(cursor_move(&mut state, &layout, center(layout.toggle)));
    assert!(state.editor_ui.projects_panel.toggle_hover);
    assert_eq!(state.editor_ui.projects_panel.hover, None);

    assert!(clear_hover(&mut state));
    assert!(!state.editor_ui.projects_panel.toggle_hover);
    assert!(!clear_hover(&mut state));
}

#[test]
fn the_dock_is_not_laid_out_when_it_is_not_shown() {
    let mut state = state_with(shop());
    state.editor_ui.document_tabs_supported = false;
    assert!(layout(&state, VIEWPORT_W, VIEWPORT_H).is_none());
}

#[test]
fn collapsing_drops_the_hover_that_belonged_to_the_open_layout() {
    let mut state = state_with(shop());
    let layout = dock(&state);
    cursor_move(&mut state, &layout, row_point(&layout, RowKind::File(0, 0)));
    toggle(&mut state);
    assert_eq!(state.editor_ui.projects_panel.hover, None);
}
