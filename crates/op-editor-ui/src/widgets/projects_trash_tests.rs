//! The Projects list's remove, create-here and "not on disk" actions: where the
//! buttons sit, what a press on them does (and does not do), and what paints.

use super::projects_panel_flow_tests::{file, project, state_with, BODY, TEXT};
use super::*;
use crate::widgets::projects_panel_layout::RowKind;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::Theme;
use op_editor_core::{FigStatus, ProjectFileKind};

const CART: &str = "/work/shop/designs/cart.op";

fn removable(label: &str, open: Option<u64>) -> op_editor_core::ProjectFileView {
    let mut view = file(label, open);
    view.removable = true;
    view
}

/// An opened project: a root file, a `designs` folder with two removable
/// designs and a `.fig`, and a `notes` folder.
fn shop() -> EditorState {
    let mut root = removable("root.op", None);
    root.folder = String::new();
    let mut cart = removable("cart.op", None);
    cart.folder = "designs".to_string();
    let mut open = removable("open.op", Some(3));
    open.folder = "designs".to_string();
    let mut fig = file("brand.fig", None);
    fig.folder = "designs".to_string();
    fig.kind = ProjectFileKind::Figma(FigStatus::NotConverted);
    let mut note = removable("todo.op", None);
    note.folder = "notes".to_string();
    let mut view = project("shop", vec![root, cart, open, fig, note], true);
    view.closable = true;
    state_with(vec![view])
}

fn laid_out(state: &EditorState) -> ProjectsPanelLayout {
    let panel = &state.editor_ui.projects_panel;
    ProjectsPanelLayout::new(&panel.projects, BODY, panel.scroll.offset)
}

fn centre_of(rect: crate::Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn on_trash(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    centre_of(layout.row(kind).and_then(|row| row.trash).expect("trash"))
}

fn on_pencil(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    centre_of(layout.row(kind).and_then(|row| row.rename).expect("pencil"))
}

fn on_label(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    let rect = layout.row(kind).expect("row").rect;
    Point2D::new(rect.origin.x + 80.0, rect.origin.y + rect.size.y / 2.0)
}

fn click(state: &mut EditorState, point: Point2D, now_ms: u64) -> ProjectsPress {
    let layout = laid_out(state);
    cursor_move(state, &layout, point);
    press(state, &layout, point, now_ms)
}

fn panel(state: &EditorState) -> &op_editor_core::editor_ui_state::ProjectsPanelState {
    &state.editor_ui.projects_panel
}

#[test]
fn fig_rows_and_folders_are_not_removable() {
    let state = shop();
    let layout = laid_out(&state);
    // Folder sub-headers and the `.fig` row carry no trash button.
    for row in &layout.rows {
        match row.kind {
            RowKind::Folder(..) => assert!(row.trash.is_none(), "folders are not removable"),
            RowKind::File(0, 3) => {
                assert!(row.trash.is_none(), "a .fig is a source, not removable");
                assert!(row.rename.is_none());
            }
            RowKind::File(..) => assert!(row.trash.is_some(), "{:?}", row.kind),
            _ => {}
        }
    }
    // Even a forced question on the `.fig` row is refused by the state.
    let mut state = state;
    assert!(!state
        .editor_ui
        .projects_panel
        .begin_trash_confirm("/work/shop/designs/brand.fig"));
}

#[test]
fn a_row_the_host_did_not_mark_removable_has_no_trash_button() {
    let mut state = shop();
    state.editor_ui.projects_panel.projects[0].files[0].removable = false;
    let layout = laid_out(&state);
    assert!(layout.row(RowKind::File(0, 0)).unwrap().trash.is_none());
    assert!(layout.row(RowKind::File(0, 0)).unwrap().rename.is_some());
}

#[test]
fn the_trash_button_asks_first_and_removes_nothing() {
    let mut state = shop();
    let layout = laid_out(&state);
    let at = on_trash(&layout, RowKind::File(0, 1));
    click(&mut state, at, 1_000);
    assert_eq!(panel(&state).trash_confirm.as_deref(), Some(CART));
    assert_eq!(panel(&state).trash_commit, None, "asking removes nothing");
    assert_eq!(panel(&state).pending_request, None);
    assert_eq!(panel(&state).rename, None);
}

#[test]
fn the_confirm_button_hands_the_path_to_the_host() {
    let mut state = shop();
    let layout = laid_out(&state);
    click(&mut state, on_trash(&layout, RowKind::File(0, 1)), 1_000);
    // The confirm button sits where the trash button was.
    let confirm = on_trash(&layout, RowKind::File(0, 1));
    assert_eq!(
        click(&mut state, confirm, 2_000),
        ProjectsPress::Claimed(Some(ProjectRowTarget::TrashFile(0, 1)))
    );
    assert_eq!(panel(&state).trash_commit.as_deref(), Some(CART));
    assert_eq!(panel(&state).trash_confirm, None);
    assert_eq!(panel(&state).pending_request, None);
}

#[test]
fn the_cancel_button_and_a_press_elsewhere_end_the_question() {
    let mut state = shop();
    let layout = laid_out(&state);
    click(&mut state, on_trash(&layout, RowKind::File(0, 1)), 1_000);
    click(&mut state, on_pencil(&layout, RowKind::File(0, 1)), 2_000);
    assert_eq!(panel(&state).trash_confirm, None, "the cross cancels");
    assert_eq!(panel(&state).trash_commit, None);
    assert_eq!(panel(&state).rename, None, "and does not start a rename");

    click(&mut state, on_trash(&layout, RowKind::File(0, 1)), 3_000);
    // A press on another row cancels and goes on to that row.
    click(&mut state, on_label(&layout, RowKind::File(0, 0)), 4_000);
    assert_eq!(panel(&state).trash_confirm, None);
    assert_eq!(
        panel(&state).trash_commit,
        None,
        "never confirms by accident"
    );
    assert!(
        panel(&state).pending_request.is_some(),
        "the other row opened"
    );
}

#[test]
fn a_press_on_the_asking_row_itself_only_cancels() {
    let mut state = shop();
    let layout = laid_out(&state);
    click(&mut state, on_trash(&layout, RowKind::File(0, 1)), 1_000);
    click(&mut state, on_label(&layout, RowKind::File(0, 1)), 2_000);
    assert_eq!(panel(&state).trash_confirm, None);
    assert_eq!(
        panel(&state).pending_request,
        None,
        "the file is not opened"
    );
}

#[test]
fn a_trash_press_that_never_saw_a_hover_counts_as_a_press_on_the_row() {
    let mut state = shop();
    let layout = laid_out(&state);
    let at = on_trash(&layout, RowKind::File(0, 1));
    press(&mut state, &layout, at, 1_000);
    assert_eq!(
        panel(&state).trash_confirm,
        None,
        "the button was not visible"
    );
    assert!(
        panel(&state).pending_request.is_some(),
        "it opened the file"
    );
}

#[test]
fn a_file_that_is_gone_shows_its_marker_and_no_actions() {
    let mut state = shop();
    state.editor_ui.projects_panel.projects[0].files[2].missing = true;
    let layout = laid_out(&state);
    let row = layout.row(RowKind::File(0, 2)).unwrap();
    assert!(row.trash.is_none() && row.rename.is_none());
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &panel(&state).projects,
        hover: Some(ProjectRowTarget::File(0, 2)),
        rename: None,
        trash_confirm: None,
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    assert!(
        backend.texts.iter().any(|(text, _)| text == "not on disk"),
        "the marker paints even while hovered: {:?}",
        backend.texts
    );
}

#[test]
fn the_asking_row_paints_the_question_and_the_two_buttons() {
    let mut state = shop();
    state.editor_ui.projects_panel.begin_trash_confirm(CART);
    let layout = laid_out(&state);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &panel(&state).projects,
        hover: None,
        rename: None,
        trash_confirm: panel(&state).trash_confirm.as_deref(),
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    let texts = &backend.texts;
    assert!(
        texts.iter().any(|(text, _)| text == "Move to Trash?"),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|(text, _)| text == "cart.op"),
        "the name gives way"
    );
}

#[test]
fn new_here_sits_on_opened_headers_and_on_folders_that_can_hold_a_design() {
    let state = shop();
    let layout = laid_out(&state);
    assert!(layout.row(RowKind::Header(0)).unwrap().new_here.is_some());
    let folder = |index| layout.row(RowKind::Folder(0, index)).unwrap();
    assert!(folder(1).new_here.is_some(), "designs/");
    assert!(folder(4).new_here.is_some(), "notes/");

    // An inferred project lists only `designs/`: anywhere else a new file
    // would never show, so no button.
    let mut state = shop();
    state.editor_ui.projects_panel.projects[0].closable = false;
    let layout = laid_out(&state);
    assert!(layout.row(RowKind::Header(0)).unwrap().new_here.is_none());
    assert!(layout
        .row(RowKind::Folder(0, 1))
        .unwrap()
        .new_here
        .is_some());
    assert!(layout
        .row(RowKind::Folder(0, 4))
        .unwrap()
        .new_here
        .is_none());
}

#[test]
fn new_here_on_a_folder_requests_a_design_in_that_folder() {
    let mut state = shop();
    let layout = laid_out(&state);
    let button = layout
        .row(RowKind::Folder(0, 1))
        .and_then(|row| row.new_here)
        .unwrap();
    click(&mut state, centre_of(button), 1_000);
    assert_eq!(
        panel(&state).pending_request,
        Some(ProjectRequest::NewDesignHere(
            "/work/shop".to_string(),
            "designs".to_string()
        ))
    );
}

#[test]
fn new_here_on_the_header_requests_a_design_in_the_project_root() {
    let mut state = shop();
    let layout = laid_out(&state);
    let button = layout
        .row(RowKind::Header(0))
        .and_then(|row| row.new_here)
        .unwrap();
    click(&mut state, centre_of(button), 1_000);
    assert_eq!(
        panel(&state).pending_request,
        Some(ProjectRequest::NewDesignHere(
            "/work/shop".to_string(),
            String::new()
        ))
    );
}

#[test]
fn a_new_here_press_that_never_saw_a_hover_is_a_press_on_the_row() {
    let mut state = shop();
    let layout = laid_out(&state);
    let button = layout
        .row(RowKind::Folder(0, 1))
        .and_then(|row| row.new_here)
        .unwrap();
    press(&mut state, &layout, centre_of(button), 1_000);
    assert!(
        matches!(
            panel(&state).pending_request,
            Some(ProjectRequest::ToggleFolder(..))
        ),
        "it folded the folder, it did not create a file"
    );
}

#[test]
fn revealing_a_row_scrolls_just_enough() {
    let mut state = shop();
    let many: Vec<_> = (0..60)
        .map(|index| file(&format!("f{index:02}.op"), None))
        .collect();
    state.editor_ui.projects_panel.projects = vec![project("shop", many, true)];
    let layout = laid_out(&state);
    let last = "/work/shop/designs/f59.op";
    assert!(reveal_file(&mut state, &layout, last));
    let layout = laid_out(&state);
    let row = layout.row(RowKind::File(0, 59)).unwrap().rect;
    assert!(
        row.origin.y + row.size.y <= BODY.origin.y + BODY.size.y + 0.01,
        "now inside the viewport"
    );
    assert!(!reveal_file(&mut state, &layout, last), "already visible");
    assert!(!reveal_file(&mut state, &layout, "/nowhere.op"));
}
