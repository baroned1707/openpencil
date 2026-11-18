//! The dock's "Open, not in a project" section: which open documents it lists,
//! that it shows with no project at all, what a press on its rows does (and
//! does not do), and what it paints.

use super::projects_open_tests::{centre, on_label, open_project, tab};
use super::projects_panel_flow_tests::{state_with, BODY, TEXT};
use super::*;
use crate::widgets::projects_panel_layout::RowKind;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::Theme;
use op_editor_core::{DocumentId, DocumentTabRequest};

/// The layout with the open documents, so the section is built.
fn laid_out(state: &EditorState) -> ProjectsPanelLayout {
    let panel = &state.editor_ui.projects_panel;
    ProjectsPanelLayout::with_tabs(
        &panel.projects,
        &state.editor_ui.document_tabs,
        BODY,
        panel.scroll.offset,
    )
}

/// Move onto `point` first, as a real pointer does, then press.
fn click(state: &mut EditorState, point: Point2D) -> ProjectsPress {
    let layout = laid_out(state);
    cursor_move(state, &layout, point);
    press(state, &layout, point, 0)
}

fn paint_texts(state: &EditorState) -> CaptureBackend {
    let layout = laid_out(state);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &state.editor_ui.projects_panel.projects,
        tabs: &state.editor_ui.document_tabs,
        hover: None,
        rename: None,
        trash_confirm: None,
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    backend
}

#[test]
fn documents_no_project_lists_get_their_own_section_after_the_projects() {
    let mut state = open_project(1);
    // Tabs: the project's file (id 1), an untitled one (id 50), a stray (id 60).
    state.editor_ui.document_tabs = vec![tab(1, "f1.op"), tab(50, "Untitled"), tab(60, "stray.op")];
    let layout = laid_out(&state);
    let kinds: Vec<RowKind> = layout.rows.iter().map(|row| row.kind).collect();
    let header = kinds
        .iter()
        .position(|k| *k == RowKind::LooseHeader)
        .expect("section header");
    assert_eq!(
        kinds[header + 1..],
        [RowKind::LooseDoc(1), RowKind::LooseDoc(2)]
    );
    assert!(
        kinds[..header]
            .iter()
            .all(|k| !matches!(k, RowKind::LooseDoc(_))),
        "after every project row"
    );
    assert!(
        !kinds.contains(&RowKind::LooseDoc(0)),
        "a project's file is not listed twice"
    );
}

#[test]
fn the_section_shows_with_no_project_at_all_and_the_list_is_not_empty() {
    let mut state = state_with(Vec::new());
    state.editor_ui.document_tabs = vec![tab(5, "Untitled")];
    let layout = laid_out(&state);
    assert!(!layout.empty && layout.empty_button.is_none());
    assert!(layout.row(RowKind::LooseDoc(0)).is_some());
    // No section at all when every document has a project row.
    let state = open_project(2);
    assert!(laid_out(&state)
        .rows
        .iter()
        .all(|row| !matches!(row.kind, RowKind::LooseHeader | RowKind::LooseDoc(_))));
}

#[test]
fn a_section_row_switches_closes_and_never_renames() {
    let mut state = state_with(Vec::new());
    state.editor_ui.document_tabs = vec![tab(5, "Untitled"), tab(6, "Untitled 2")];
    let layout = laid_out(&state);
    let row = layout.row(RowKind::LooseDoc(1)).unwrap();
    assert!(
        row.rename.is_none() && row.trash.is_none(),
        "path-keyed actions do not apply"
    );
    // A press shows the document.
    let at = on_label(&layout, RowKind::LooseDoc(1));
    click(&mut state, at);
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(6)))
    );
    // A second quick press is not a rename.
    state.editor_ui.pending_document_request = None;
    let layout = laid_out(&state);
    press(&mut state, &layout, at, 100);
    assert_eq!(state.editor_ui.projects_panel.rename, None);
    // Its close button closes it.
    let close = centre(row.close.unwrap());
    click(&mut state, close);
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(6)))
    );
    // So does a middle press.
    state.editor_ui.pending_document_request = None;
    let layout = laid_out(&state);
    middle_press(&mut state, &layout, on_label(&layout, RowKind::LooseDoc(0)));
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(DocumentId(5)))
    );
}

#[test]
fn the_section_paints_its_label_rows_and_open_marks() {
    let mut state = state_with(Vec::new());
    state.editor_ui.document_tabs = vec![tab(5, "Untitled"), tab(6, "stray.op")];
    let painted = paint_texts(&state);
    let texts: Vec<&str> = painted.texts.iter().map(|(t, _)| t.as_str()).collect();
    for expected in [
        "Open, not in a project",
        "Untitled",
        "stray.op",
        "\u{2318}1",
        "\u{2318}2",
    ] {
        assert!(texts.contains(&expected), "{expected} missing in {texts:?}");
    }
}
