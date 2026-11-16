//! Flow tests for the Projects dock's list: what a click turns into,
//! scrolling, and the hover contract.

use super::*;
use crate::widgets::projects_panel_layout::{RowKind, FILE_HEIGHT, HEADER_HEIGHT};
use crate::Rect;
use op_editor_core::{DocumentId, DocumentTabStatus, ProjectFileView, ProjectView};

/// A rail-shaped area the list tests place their body in.
pub(crate) const RAIL: Rect = Rect {
    origin: Point2D { x: 0.0, y: 48.0 },
    size: Point2D { x: 280.0, y: 600.0 },
};

/// The list body inside [`RAIL`]: below a 44 px header band.
pub(crate) const BODY: Rect = Rect {
    origin: Point2D { x: 0.0, y: 92.0 },
    size: Point2D { x: 280.0, y: 556.0 },
};

pub(crate) fn file(label: &str, open: Option<u64>) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/designs/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
    }
}

pub(crate) fn project(name: &str, files: Vec<ProjectFileView>, expanded: bool) -> ProjectView {
    ProjectView {
        root: format!("/work/{name}"),
        name: name.to_string(),
        files,
        expanded,
    }
}

/// A state whose host supports tabs and holds `projects`.
pub(crate) fn state_with(projects: Vec<ProjectView>) -> EditorState {
    let mut state = EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.projects_panel.projects = projects;
    state
}

pub(crate) fn shop() -> Vec<ProjectView> {
    vec![
        project(
            "shop",
            vec![file("checkout.op", Some(7)), file("cart.op", None)],
            true,
        ),
        project("blog", vec![file("home.op", None)], false),
    ]
}

fn laid_out(state: &EditorState) -> ProjectsPanelLayout {
    let panel = &state.editor_ui.projects_panel;
    ProjectsPanelLayout::new(&panel.projects, BODY, panel.scroll.offset)
}

fn centre(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    let rect = layout.row(kind).expect("row is laid out").rect;
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0 - 30.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

/// Move onto `point` first, as a real pointer always does, then press.
fn click(state: &mut EditorState, layout: &ProjectsPanelLayout, point: Point2D) -> ProjectsPress {
    cursor_move(state, layout, point);
    press(state, layout, point)
}

#[test]
fn click_on_open_file_requests_switch() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let at = centre(&layout, RowKind::File(0, 0));
    assert_eq!(
        click(&mut state, &layout, at),
        ProjectsPress::Claimed(Some(ProjectRowTarget::File(0, 0)))
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::SwitchTo(DocumentId(7)))
    );
}

#[test]
fn click_on_unopened_file_requests_open() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let at = centre(&layout, RowKind::File(0, 1));
    click(&mut state, &layout, at);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/designs/cart.op".to_string()
        ))
    );
}

#[test]
fn click_on_header_requests_toggle() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let at = centre(&layout, RowKind::Header(1));
    click(&mut state, &layout, at);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ToggleProject("/work/blog".to_string()))
    );
    // The host owns the flag; the tab never flips it.
    assert!(!state.editor_ui.projects_panel.projects[1].expanded);
}

#[test]
fn plus_requests_new_design_in_that_project() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let plus = layout.row(RowKind::Header(0)).unwrap().plus.unwrap();
    let at = Point2D::new(
        plus.origin.x + plus.size.x / 2.0,
        plus.origin.y + plus.size.y / 2.0,
    );
    click(&mut state, &layout, at);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::NewDesignIn("/work/shop".to_string()))
    );
}

#[test]
fn a_press_with_no_prior_hover_never_hits_the_hidden_plus() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let plus = layout.row(RowKind::Header(0)).unwrap().plus.unwrap();
    let at = Point2D::new(plus.origin.x + 4.0, plus.origin.y + 4.0);
    press(&mut state, &layout, at);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ToggleProject("/work/shop".to_string())),
        "an unseen button is a press on the header"
    );
}

#[test]
fn an_empty_project_offers_new_design_in_place_of_files() {
    let mut state = state_with(vec![project("fresh", Vec::new(), true)]);
    let layout = laid_out(&state);
    let at = centre(&layout, RowKind::NewDesign(0));
    click(&mut state, &layout, at);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::NewDesignIn("/work/fresh".to_string()))
    );
}

#[test]
fn collapsed_project_hides_its_files() {
    let state = state_with(shop());
    let layout = laid_out(&state);
    assert!(layout.row(RowKind::File(0, 0)).is_some());
    assert!(
        layout.row(RowKind::File(1, 0)).is_none(),
        "the collapsed blog project lists no files"
    );
    // Header, two files, collapsed header.
    assert_eq!(layout.rows.len(), 4);
}

#[test]
fn a_press_on_bare_body_is_claimed_without_a_request() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let below = Point2D::new(40.0, RAIL.origin.y + RAIL.size.y - 10.0);
    assert_eq!(
        click(&mut state, &layout, below),
        ProjectsPress::Claimed(None)
    );
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
    let outside = Point2D::new(500.0, 100.0);
    assert_eq!(
        press(&mut state, &layout, outside),
        ProjectsPress::Missed,
        "points off the list fall through"
    );
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
fn scroll_clamps_to_content() {
    let mut state = state_with(many_projects());
    let inside = Point2D::new(40.0, RAIL.origin.y + 60.0);
    let max = laid_out(&state).max_scroll();
    assert!(max > 0.0, "twelve projects overflow the list");

    // Wheel convention: a negative delta scrolls down the list. The layout
    // is rebuilt per event, exactly as the hosts do.
    let layout = laid_out(&state);
    assert_eq!(
        scroll(&mut state, Some(&layout), inside, -100_000.0),
        Some(true)
    );
    assert_eq!(state.editor_ui.projects_panel.scroll.offset, max);
    let layout = laid_out(&state);
    assert_eq!(
        scroll(&mut state, Some(&layout), inside, -50.0),
        Some(false),
        "already at the bottom"
    );
    assert_eq!(
        scroll(&mut state, Some(&layout), inside, 100_000.0),
        Some(true)
    );
    assert_eq!(state.editor_ui.projects_panel.scroll.offset, 0.0);
    let layout = laid_out(&state);
    assert_eq!(
        scroll(&mut state, Some(&layout), Point2D::new(900.0, 10.0), -10.0),
        None,
        "a wheel off the list keeps looking"
    );
}

#[test]
fn short_content_does_not_scroll() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    assert_eq!(layout.max_scroll(), 0.0);
    let inside = Point2D::new(40.0, layout.panel.origin.y + 20.0);
    assert_eq!(
        scroll(&mut state, Some(&layout), inside, -80.0),
        Some(false)
    );
}

#[test]
fn a_scrolled_list_hit_tests_the_rows_it_paints() {
    let mut state = state_with(many_projects());
    state.editor_ui.projects_panel.scroll.offset = 3.0 * HEADER_HEIGHT + 10.0 * FILE_HEIGHT;
    let layout = laid_out(&state);
    let top = Point2D::new(40.0, layout.panel.origin.y + 1.0);
    // Whatever row sits under the top edge is the one a click there gets.
    let target = layout.hit(top);
    let painted = layout
        .rows
        .iter()
        .find(|row| row.rect.contains(top))
        .map(|row| row.kind);
    assert!(matches!(
        (target, painted),
        (Some(ProjectRowTarget::Header(_)), Some(RowKind::Header(_)))
            | (Some(ProjectRowTarget::File(..)), Some(RowKind::File(..)))
    ));
}

#[test]
fn hover_tracks_the_row_under_the_cursor_and_clears() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let at = centre(&layout, RowKind::File(0, 1));
    assert!(cursor_move(&mut state, &layout, at));
    assert_eq!(
        state.editor_ui.projects_panel.hover,
        Some(ProjectRowTarget::File(0, 1))
    );
    assert!(
        !cursor_move(&mut state, &layout, at),
        "no change, no repaint"
    );
    assert!(clear_hover(&mut state));
    assert!(!clear_hover(&mut state));
}
