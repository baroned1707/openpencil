//! The dock's open-document features: the close (x) of an open file row, the
//! middle-click close, the Cmd+digit open mark, and the "Open, not in a
//! project" section.

use super::projects_panel_flow_tests::{file, project, state_with, BODY, TEXT};
use super::*;
use crate::widgets::projects_panel_layout::RowKind;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::Theme;
use op_editor_core::{DocumentId, DocumentTabStatus, DocumentTabView};

pub(super) fn tab(id: u64, label: &str) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: String::new(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
    }
}

/// A project whose files `a.op`, `b.op`, ... are open as documents 1, 2, ...,
/// plus one closed file, with `tabs` mirroring the open ones in order.
pub(super) fn open_project(open: u64) -> EditorState {
    let mut files: Vec<_> = (1..=open)
        .map(|n| file(&format!("f{n}.op"), Some(n)))
        .collect();
    files.push(file("closed.op", None));
    let mut state = state_with(vec![project("shop", files, true)]);
    state.editor_ui.document_tabs = (1..=open).map(|n| tab(n, &format!("f{n}.op"))).collect();
    state
}

fn laid_out(state: &EditorState) -> ProjectsPanelLayout {
    let panel = &state.editor_ui.projects_panel;
    ProjectsPanelLayout::new(&panel.projects, BODY, panel.scroll.offset)
}

pub(super) fn centre(rect: crate::Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

pub(super) fn on_label(layout: &ProjectsPanelLayout, kind: RowKind) -> Point2D {
    let rect = layout.row(kind).expect("row").rect;
    Point2D::new(rect.origin.x + 80.0, rect.origin.y + rect.size.y / 2.0)
}

/// Move onto `point` first, as a real pointer does, then press.
fn click(state: &mut EditorState, point: Point2D) -> ProjectsPress {
    let layout = laid_out(state);
    cursor_move(state, &layout, point);
    press(state, &layout, point, 0)
}

fn paint_texts(state: &EditorState, hover: Option<ProjectRowTarget>) -> CaptureBackend {
    let layout = laid_out(state);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &state.editor_ui.projects_panel.projects,
        tabs: &state.editor_ui.document_tabs,
        hover,
        rename: None,
        trash_confirm: None,
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    backend
}

#[test]
fn an_open_row_offers_close_in_place_of_trash_and_a_closed_row_keeps_trash() {
    let mut state = open_project(2);
    for view in &mut state.editor_ui.projects_panel.projects[0].files {
        view.removable = true;
    }
    let layout = laid_out(&state);
    let open = layout.row(RowKind::File(0, 0)).unwrap();
    assert!(
        open.close.is_some() && open.trash.is_none(),
        "open: close, no trash"
    );
    let closed = layout.row(RowKind::File(0, 2)).unwrap();
    assert!(
        closed.trash.is_some() && closed.close.is_none(),
        "closed: trash, no close"
    );
    // The close button sits where the trash button sits on a closed row.
    assert_eq!(
        open.close.map(|r| r.origin.x),
        closed.trash.map(|r| r.origin.x)
    );
}

#[test]
fn pressing_close_on_an_open_row_requests_a_document_close_not_a_trash() {
    let mut state = open_project(2);
    state.editor_ui.projects_panel.projects[0].files[1].removable = true;
    let layout = laid_out(&state);
    let at = centre(layout.row(RowKind::File(0, 1)).unwrap().close.unwrap());
    assert_eq!(
        click(&mut state, at),
        ProjectsPress::Claimed(Some(ProjectRowTarget::CloseFile(0, 1)))
    );
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(2)))
    );
    let panel = &state.editor_ui.projects_panel;
    assert_eq!(panel.trash_confirm, None, "no trash question");
    assert_eq!(panel.pending_request, None, "no project request either");
}

#[test]
fn a_close_press_that_never_saw_a_hover_is_a_press_on_the_row() {
    let mut state = open_project(2);
    let layout = laid_out(&state);
    let at = centre(layout.row(RowKind::File(0, 1)).unwrap().close.unwrap());
    press(&mut state, &layout, at, 0);
    assert_eq!(
        state.editor_ui.pending_document_request, None,
        "nothing closed"
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::SwitchTo(DocumentId(2)))
    );
}

#[test]
fn an_open_file_that_is_gone_from_disk_can_still_be_closed() {
    let mut state = open_project(2);
    state.editor_ui.projects_panel.projects[0].files[0].missing = true;
    let layout = laid_out(&state);
    assert!(layout.row(RowKind::File(0, 0)).unwrap().close.is_some());
}

#[test]
fn a_middle_press_closes_an_open_file_and_only_that() {
    let mut state = open_project(2);
    let layout = laid_out(&state);
    // Open row: closes it.
    let on_open = on_label(&layout, RowKind::File(0, 1));
    assert_eq!(
        middle_press(&mut state, &layout, on_open),
        ProjectsPress::Claimed(Some(ProjectRowTarget::File(0, 1)))
    );
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(2)))
    );
    // Closed row: swallowed, nothing requested.
    state.editor_ui.pending_document_request = None;
    let on_closed = on_label(&layout, RowKind::File(0, 2));
    assert!(matches!(
        middle_press(&mut state, &layout, on_closed),
        ProjectsPress::Claimed(_)
    ));
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
    // Off the list: not ours.
    assert_eq!(
        middle_press(&mut state, &layout, Point2D::new(-5.0, -5.0)),
        ProjectsPress::Missed
    );
}

#[test]
fn the_open_mark_is_the_digit_the_shortcut_uses_and_a_dot_past_it() {
    // Eleven open documents: 1..8, then 9 only on the last; 9th and 10th get a dot.
    let state = open_project(11);
    let painted = paint_texts(&state, None);
    let badges: Vec<&str> = painted
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .filter(|text| text.starts_with('\u{2318}'))
        .collect();
    assert_eq!(
        badges,
        [
            "\u{2318}1",
            "\u{2318}2",
            "\u{2318}3",
            "\u{2318}4",
            "\u{2318}5",
            "\u{2318}6",
            "\u{2318}7",
            "\u{2318}8",
            "\u{2318}9"
        ],
        "digits follow the shortcut order; the 9th and 10th rows have none"
    );
    let dots = painted
        .round_fills
        .iter()
        .filter(|(rect, _, _)| (rect.size.x - 4.0).abs() < 0.01)
        .count();
    assert_eq!(
        dots, 2,
        "the two documents no chord reaches get a plain dot"
    );
    // The last open document carries the 9 (it is what Cmd+9 reaches): row 11.
    let last_row = laid_out(&state).row(RowKind::File(0, 10)).unwrap().rect;
    let nine = painted
        .texts
        .iter()
        .find(|(text, _)| text == "\u{2318}9")
        .unwrap();
    assert!(nine.1.y > last_row.origin.y && nine.1.y < last_row.origin.y + last_row.size.y);
}

#[test]
fn a_single_open_document_has_no_digit_because_the_chord_does_nothing() {
    let state = open_project(1);
    let painted = paint_texts(&state, None);
    assert!(!painted
        .texts
        .iter()
        .any(|(text, _)| text.starts_with('\u{2318}')));
}

#[test]
fn the_open_mark_gives_way_to_the_buttons_when_the_row_is_hovered() {
    let state = open_project(3);
    let painted = paint_texts(&state, Some(ProjectRowTarget::File(0, 0)));
    let badge_on_row = |row: usize| {
        let rect = laid_out(&state).row(RowKind::File(0, row)).unwrap().rect;
        painted.texts.iter().any(|(text, at)| {
            text.starts_with('\u{2318}')
                && at.y > rect.origin.y
                && at.y < rect.origin.y + rect.size.y
        })
    };
    assert!(!badge_on_row(0), "hovered row shows its buttons");
    assert!(badge_on_row(1), "other rows keep their mark");
}
