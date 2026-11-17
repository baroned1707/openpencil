//! The Projects list's inline rename: how it starts (double click, pencil),
//! what settles it (Enter, Escape, a press elsewhere) and what paints.

use super::projects_panel_flow_tests::{file, project, shop, state_with, BODY, TEXT};
use super::*;
use crate::widgets::projects_panel::rename_field_rect;
use crate::widgets::projects_panel_layout::RowKind;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::Theme;
use op_editor_core::{
    FigStatus, ProjectFileKind, ProjectRenameRequest, ProjectRowTarget, RenameNameError,
};

fn laid_out(state: &EditorState) -> ProjectsPanelLayout {
    let panel = &state.editor_ui.projects_panel;
    ProjectsPanelLayout::new(&panel.projects, BODY, panel.scroll.offset)
}

/// The middle of a row's label area (clear of the pencil at the right end).
fn on_row(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    let rect = layout.row(kind).expect("row is laid out").rect;
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0 - 30.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn on_pencil(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    let button = layout
        .row(kind)
        .and_then(|row| row.rename)
        .expect("the row has a pencil");
    Point2D::new(
        button.origin.x + button.size.x / 2.0,
        button.origin.y + button.size.y / 2.0,
    )
}

/// Move onto `point` first, as a real pointer always does, then press.
fn click(state: &mut EditorState, point: Point2D, now_ms: u64) -> ProjectsPress {
    let layout = laid_out(state);
    cursor_move(state, &layout, point);
    press(state, &layout, point, now_ms)
}

fn renaming(state: &EditorState) -> Option<&str> {
    state
        .editor_ui
        .projects_panel
        .rename
        .as_ref()
        .map(|edit| edit.path.as_str())
}

#[test]
fn a_second_press_on_the_same_design_starts_a_rename() {
    let mut state = state_with(shop());
    let at = on_row(&laid_out(&state), RowKind::File(0, 1));
    click(&mut state, at, 1_000);
    assert_eq!(renaming(&state), None, "one press only opens it");
    assert!(state.editor_ui.projects_panel.pending_request.is_some());
    state.editor_ui.projects_panel.pending_request = None;

    click(&mut state, at, 1_300);
    assert_eq!(renaming(&state), Some("/work/shop/designs/cart.op"));
    assert_eq!(
        state.editor_ui.projects_panel.pending_request, None,
        "the second press asks the host for nothing"
    );
    let edit = state.editor_ui.projects_panel.rename.as_ref().unwrap();
    assert_eq!(edit.input.text(), "cart", "no extension in the field");
    assert_eq!(edit.input.highlight_range(), Some((0, 4)), "stem selected");
}

#[test]
fn two_presses_too_far_apart_or_on_different_rows_do_not() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let cart = on_row(&layout, RowKind::File(0, 1));
    let checkout = on_row(&layout, RowKind::File(0, 0));
    click(&mut state, cart, 1_000);
    click(&mut state, cart, 1_000 + DOUBLE_CLICK_MS);
    assert_eq!(renaming(&state), None, "the window is shorter than 400 ms");
    click(&mut state, checkout, 2_000);
    click(&mut state, cart, 2_100);
    assert_eq!(renaming(&state), None, "another row in between");
}

#[test]
fn the_pencil_starts_a_rename_without_opening_the_file() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let pencil = on_pencil(&layout, RowKind::File(0, 1));
    click(&mut state, pencil, 1_000);
    assert_eq!(renaming(&state), Some("/work/shop/designs/cart.op"));
    assert_eq!(
        state.editor_ui.projects_panel.pending_request, None,
        "a closed file stays closed"
    );
}

#[test]
fn a_pencil_press_that_never_saw_a_hover_counts_as_a_press_on_the_row() {
    let mut state = state_with(shop());
    let layout = laid_out(&state);
    let pencil = on_pencil(&layout, RowKind::File(0, 1));
    // Straight to press: the button was never visible.
    press(&mut state, &layout, pencil, 1_000);
    assert_eq!(renaming(&state), None);
    assert!(
        state.editor_ui.projects_panel.pending_request.is_some(),
        "it opened the file like any press on the row"
    );
}

#[test]
fn enter_commits_a_valid_name_and_the_host_gets_it() {
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    op_editor_core::project_rename_keyboard::paste(&mut state, "basket", 1);
    assert_eq!(
        op_editor_core::project_rename_keyboard::confirm(&mut state),
        Some(op_editor_core::RenameConfirm::Submitted)
    );
    assert_eq!(
        state.editor_ui.projects_panel.rename_commit,
        Some(ProjectRenameRequest {
            path: "/work/shop/designs/cart.op".to_string(),
            new_stem: "basket".to_string()
        })
    );
}

#[test]
fn escape_cancels_and_nothing_is_requested() {
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    op_editor_core::project_rename_keyboard::paste(&mut state, "basket", 1);
    assert!(op_editor_core::project_rename_keyboard::cancel(&mut state));
    assert_eq!(renaming(&state), None);
    assert_eq!(state.editor_ui.projects_panel.rename_commit, None);
}

#[test]
fn a_press_inside_the_field_keeps_the_rename() {
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    let layout = laid_out(&state);
    let row = layout.row(RowKind::File(0, 1)).unwrap();
    let field = rename_field_rect(row, &state.editor_ui.projects_panel.projects[0].files[1]);
    let inside = Point2D::new(field.origin.x + 20.0, field.origin.y + field.size.y / 2.0);
    assert_eq!(
        press(&mut state, &layout, inside, 1_000),
        ProjectsPress::Claimed(None)
    );
    assert_eq!(renaming(&state), Some("/work/shop/designs/cart.op"));
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
}

#[test]
fn click_away_commits_only_a_valid_name() {
    // Valid: the name is kept and the press goes on to its own row.
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    op_editor_core::project_rename_keyboard::paste(&mut state, "basket", 1);
    let other = on_row(&laid_out(&state), RowKind::File(0, 0));
    click(&mut state, other, 5_000);
    assert_eq!(
        state
            .editor_ui
            .projects_panel
            .rename_commit
            .as_ref()
            .map(|r| r.new_stem.as_str()),
        Some("basket")
    );
    assert!(
        renaming(&state).is_some(),
        "the field closes when the host answers, not before"
    );
    assert!(
        state.editor_ui.projects_panel.pending_request.is_some(),
        "the press still did what it was aimed at"
    );

    // Invalid: dropped.
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    op_editor_core::project_rename_keyboard::paste(&mut state, "a:b", 1);
    let other = on_row(&laid_out(&state), RowKind::File(0, 0));
    click(&mut state, other, 5_000);
    assert_eq!(renaming(&state), None);
    assert_eq!(state.editor_ui.projects_panel.rename_commit, None);

    // A name another file in the folder has: dropped too.
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    op_editor_core::project_rename_keyboard::paste(&mut state, "CHECKOUT", 1);
    let other = on_row(&laid_out(&state), RowKind::File(0, 0));
    click(&mut state, other, 5_000);
    assert_eq!(state.editor_ui.projects_panel.rename_commit, None);
}

#[test]
fn fig_rows_and_folders_are_not_renamable() {
    let mut fig = file("brand.fig", None);
    fig.kind = ProjectFileKind::Figma(FigStatus::NotConverted);
    fig.folder = String::new();
    let mut nested = file("deep.op", None);
    nested.folder = "sub".to_string();
    let mut state = state_with(vec![project(
        "shop",
        vec![fig, file("a.op", None), nested],
        true,
    )]);
    let layout = laid_out(&state);
    // A `.fig` row has no pencil, and two presses convert it, never rename it.
    assert!(layout.row(RowKind::File(0, 0)).unwrap().rename.is_none());
    let on_fig = on_row(&layout, RowKind::File(0, 0));
    click(&mut state, on_fig, 1_000);
    click(&mut state, on_fig, 1_100);
    assert_eq!(renaming(&state), None);
    // A folder sub-header: two presses fold and unfold it, nothing renames.
    let layout = laid_out(&state);
    let on_folder = on_row(&layout, RowKind::Folder(0, 2));
    click(&mut state, on_folder, 2_000);
    click(&mut state, on_folder, 2_100);
    assert_eq!(renaming(&state), None);
    assert!(!state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/brand.fig", 0));
    // A `.pen` file (open from the project) has no pencil either.
    let state = state_with(vec![project("shop", vec![file("old.pen", None)], true)]);
    assert!(laid_out(&state)
        .row(RowKind::File(0, 0))
        .unwrap()
        .rename
        .is_none());
}

fn paint(state: &EditorState, hover: Option<ProjectRowTarget>) -> CaptureBackend {
    let layout = laid_out(state);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &state.editor_ui.projects_panel.projects,
        hover,
        rename: state.editor_ui.projects_panel.rename.as_ref(),
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    backend
}

#[test]
fn the_edit_row_paints_the_stem_not_the_file_name() {
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    let backend = paint(&state, None);
    let texts: Vec<&str> = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .collect();
    assert!(texts.contains(&"cart"), "the field's text: {texts:?}");
    assert!(
        !texts.contains(&"cart.op"),
        "the label is replaced: {texts:?}"
    );
    assert!(texts.contains(&"checkout.op"), "other rows stay: {texts:?}");
}

#[test]
fn an_error_shows_one_line_under_the_field() {
    let mut state = state_with(shop());
    state
        .editor_ui
        .projects_panel
        .begin_rename("/work/shop/designs/cart.op", 0);
    let says = |backend: &CaptureBackend, line: &str| {
        // The line may be ellipsized to fit the dock; its start always shows.
        let head: String = line.chars().take(12).collect();
        backend.texts.iter().any(|(t, _)| t.starts_with(&head))
    };
    assert!(!says(&paint(&state, None), TEXT.rename_exists));
    state
        .editor_ui
        .projects_panel
        .reject_rename("/work/shop/designs/cart.op", RenameNameError::Taken);
    let backend = paint(&state, None);
    assert!(says(&backend, TEXT.rename_exists));
    state
        .editor_ui
        .projects_panel
        .reject_rename("/work/shop/designs/cart.op", RenameNameError::Separator);
    let backend = paint(&state, None);
    assert!(says(&backend, TEXT.rename_invalid));
    // The line is painted over the row below: no row moved.
    let layout = laid_out(&state);
    let cart = layout.row(RowKind::File(0, 1)).unwrap().rect;
    assert_eq!(
        cart.size.y,
        crate::widgets::projects_panel_layout::FILE_HEIGHT
    );
}

#[test]
fn a_hovered_design_row_shows_the_pencil_and_a_fig_row_never_does() {
    use crate::widgets::icons::Icon;
    let pencils = |backend: &CaptureBackend| {
        backend
            .svg_strokes
            .iter()
            .filter(|(d, ..)| Icon::Pencil.paths().contains(&d.as_str()))
            .count()
    };
    let state = state_with(shop());
    assert_eq!(pencils(&paint(&state, None)), 0, "a calm row has none");
    let hovered = pencils(&paint(&state, Some(ProjectRowTarget::File(0, 1))));
    assert!(hovered > 0, "the pencil is drawn on the hovered row");
    assert_eq!(
        pencils(&paint(&state, Some(ProjectRowTarget::RenameFile(0, 1)))),
        hovered
    );

    let mut fig = file("brand.fig", None);
    fig.kind = ProjectFileKind::Figma(FigStatus::NotConverted);
    let state = state_with(vec![project("shop", vec![fig], true)]);
    assert_eq!(
        pencils(&paint(&state, Some(ProjectRowTarget::File(0, 0)))),
        0
    );
}
