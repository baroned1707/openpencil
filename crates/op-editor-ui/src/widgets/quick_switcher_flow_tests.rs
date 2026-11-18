use super::*;
use crate::widgets::quick_switcher::QuickSwitcherHit;
use op_editor_core::{
    DocumentId, DocumentTabRequest, DocumentTabStatus, DocumentTabView, ProjectFileKind,
    ProjectFileView, ProjectRequest, ProjectView,
};

const W: f32 = 1440.0;

fn state() -> EditorState {
    let mut state = EditorState::default();
    let ui = &mut state.editor_ui;
    ui.document_tabs_supported = true;
    ui.document_tabs = vec![DocumentTabView {
        id: DocumentId(1),
        label: "home.op".to_string(),
        project: "shop".to_string(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: true,
    }];
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![ProjectFileView {
            path: "/work/shop/pricing.op".to_string(),
            label: "pricing.op".to_string(),
            open_id: None,
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: false,
            folder: String::new(),
            kind: ProjectFileKind::Design,
            missing: false,
            removable: false,
        }],
        ..ProjectView::default()
    }];
    state
}

fn row_point(state: &EditorState, index: usize) -> Point2D {
    let layout = layout(state, W).expect("open");
    let rect = layout
        .rows
        .iter()
        .find(|(i, _)| *i == index)
        .expect("visible row")
        .1;
    Point2D::new(rect.origin.x + 20.0, rect.origin.y + rect.size.y / 2.0)
}

#[test]
fn a_closed_switcher_owns_nothing() {
    let mut state = state();
    assert!(layout(&state, W).is_none());
    assert!(!press(&mut state, W, Point2D::new(10.0, 10.0)));
    assert_eq!(cursor_move(&mut state, W, Point2D::new(10.0, 10.0)), None);
    assert_eq!(
        scroll(&mut state, 5.0),
        None,
        "the wheel still reaches the canvas"
    );
}

#[test]
fn an_open_switcher_swallows_a_press_outside_and_closes() {
    let mut state = state();
    switcher::open(&mut state, 0);
    let canvas_click = Point2D::new(200.0, 500.0);
    assert!(
        press(&mut state, W, canvas_click),
        "claimed: the canvas never sees it"
    );
    assert!(!switcher::is_active(&state), "an outside press closes it");
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
}

#[test]
fn a_press_on_the_card_chrome_swallows_and_keeps_it_open() {
    let mut state = state();
    switcher::open(&mut state, 0);
    let card = layout(&state, W).unwrap().card;
    assert!(press(
        &mut state,
        W,
        Point2D::new(card.origin.x + 2.0, card.origin.y + 2.0)
    ));
    assert!(switcher::is_active(&state));
}

#[test]
fn clicking_a_row_chooses_it() {
    let mut state = state();
    switcher::open(&mut state, 0);
    // Row 0 is the open document, row 1 the closed file.
    assert!({
        let at = row_point(&state, 1);
        press(&mut state, W, at)
    });
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/pricing.op".to_string()
        ))
    );
    assert!(!switcher::is_active(&state));

    switcher::open(&mut state, 0);
    assert!({
        let at = row_point(&state, 0);
        press(&mut state, W, at)
    });
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(1)))
    );
}

#[test]
fn hovering_a_row_highlights_it() {
    let mut state = state();
    switcher::open(&mut state, 0);
    let hover = row_point(&state, 1);
    assert_eq!(cursor_move(&mut state, W, hover), Some(true));
    assert_eq!(state.editor_ui.projects_panel.switcher.selected, 1);
    assert_eq!(cursor_move(&mut state, W, hover), Some(false), "unchanged");
    // Off the card the pointer is still the switcher's, and changes nothing.
    assert_eq!(
        cursor_move(&mut state, W, Point2D::new(5.0, 500.0)),
        Some(false)
    );
    assert_eq!(
        layout(&state, W).unwrap().hit(Point2D::new(5.0, 500.0)),
        QuickSwitcherHit::Outside
    );
}

#[test]
fn the_wheel_is_swallowed_while_open_even_with_nothing_to_scroll() {
    let mut state = state();
    switcher::open(&mut state, 0);
    assert_eq!(scroll(&mut state, -3.0), Some(false));
}
