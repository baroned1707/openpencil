use super::*;
use crate::document_tabs::{DocumentId, DocumentTabStatus, DocumentTabView};
use crate::project_tree::{ProjectFileKind, ProjectFileView, ProjectView};

fn tab(id: u64, label: &str) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: "shop".to_string(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: id == 1,
    }
}

fn file(label: &str) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
        folder: String::new(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

/// Two open documents (`home.op`, `cart.op`) and two closed files in `shop`.
fn state() -> EditorState {
    let mut state = EditorState::default();
    let ui = &mut state.editor_ui;
    ui.document_tabs_supported = true;
    ui.document_tabs = vec![tab(1, "home.op"), tab(2, "cart.op")];
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![file("pricing.op"), file("checkout.op")],
        ..ProjectView::default()
    }];
    state
}

fn type_str(state: &mut EditorState, text: &str) {
    for c in text.chars() {
        assert_eq!(super::text(state, c, 0), Some(true));
    }
}

#[test]
fn it_opens_only_where_the_dock_exists() {
    let mut state = state();
    assert!(open(&mut state, 0) && is_active(&state));
    close(&mut state);
    state.editor_ui.document_tabs_supported = false;
    assert!(
        !open(&mut state, 0),
        "a single-document host has no switcher"
    );
    assert!(!is_active(&state));
}

#[test]
fn every_key_is_the_switchers_while_it_is_open_and_nobodys_when_closed() {
    let mut state = state();
    assert_eq!(super::text(&mut state, 'a', 0), None);
    assert_eq!(backspace(&mut state, 0), None);
    assert_eq!(delete_forward(&mut state, 0), None);
    assert_eq!(confirm(&mut state), None);
    assert!(!move_selection(&mut state, 1) && !move_caret(&mut state, true, false, 0));
    assert!(!cancel(&mut state));
    open(&mut state, 0);
    // Even a rejected control character is swallowed.
    assert_eq!(super::text(&mut state, '\n', 0), Some(false));
}

#[test]
fn typing_narrows_the_rows_and_resets_the_selection() {
    let mut state = state();
    open(&mut state, 0);
    assert_eq!(rows(&state).len(), 4);
    move_selection(&mut state, 1);
    assert_eq!(state.editor_ui.projects_panel.switcher.selected, 1);
    type_str(&mut state, "che");
    let labels: Vec<String> = rows(&state).into_iter().map(|row| row.label).collect();
    assert_eq!(labels, ["checkout.op"]);
    assert_eq!(state.editor_ui.projects_panel.switcher.selected, 0);
    assert_eq!(backspace(&mut state, 0), Some(true));
    assert_eq!(state.editor_ui.projects_panel.switcher.input.text(), "ch");
}

#[test]
fn enter_on_an_open_document_requests_a_switch_and_closes() {
    let mut state = state();
    open(&mut state, 0);
    type_str(&mut state, "cart");
    assert_eq!(confirm(&mut state), Some(true));
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(2)))
    );
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
    assert!(!is_active(&state));
}

#[test]
fn enter_on_a_file_that_is_not_open_requests_open_file() {
    let mut state = state();
    open(&mut state, 0);
    type_str(&mut state, "pricing");
    assert_eq!(confirm(&mut state), Some(true));
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/pricing.op".to_string()
        ))
    );
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert!(!is_active(&state));
}

#[test]
fn enter_with_no_rows_changes_nothing_and_stays_open() {
    let mut state = state();
    open(&mut state, 0);
    type_str(&mut state, "zzzz");
    assert_eq!(confirm(&mut state), Some(false));
    assert!(is_active(&state));
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
}

#[test]
fn escape_closes_without_choosing() {
    let mut state = state();
    open(&mut state, 0);
    type_str(&mut state, "cart");
    assert!(cancel(&mut state));
    assert!(!is_active(&state));
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
    // Reopening starts clean.
    open(&mut state, 0);
    assert_eq!(state.editor_ui.projects_panel.switcher.input.text(), "");
}

#[test]
fn a_clicked_row_is_chosen_and_a_stale_index_is_ignored() {
    let mut state = state();
    open(&mut state, 0);
    assert!(!choose(&mut state, 9), "no such row: stays open");
    assert!(is_active(&state));
    assert!(choose(&mut state, 3));
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenFile(
            "/work/shop/checkout.op".to_string()
        ))
    );
}

#[test]
fn the_query_has_the_usual_editing_keys() {
    let mut state = state();
    open(&mut state, 0);
    type_str(&mut state, "abc");
    assert!(move_caret(&mut state, false, true, 0));
    assert_eq!(selected_text(&state).as_deref(), Some("c"));
    assert!(select_all(&mut state, 0));
    assert_eq!(selected_text(&state).as_deref(), Some("abc"));
    assert_eq!(paste(&mut state, "x\ny", 0), Some(true));
    assert_eq!(state.editor_ui.projects_panel.switcher.input.text(), "xy");
    assert!(move_caret_to_edge(&mut state, false, false, 0));
    assert_eq!(delete_forward(&mut state, 0), Some(true));
    assert_eq!(state.editor_ui.projects_panel.switcher.input.text(), "y");
}

#[test]
fn hover_selects_and_the_wheel_scrolls_without_moving_the_selection() {
    let mut state = state();
    open(&mut state, 0);
    assert!(select_row(&mut state, 2));
    assert!(!select_row(&mut state, 2), "unchanged");
    assert_eq!(state.editor_ui.projects_panel.switcher.selected, 2);
    assert!(!scroll(&mut state, 3), "four rows fit: nothing to scroll");
}

#[test]
fn it_is_the_text_input_that_owns_the_keyboard() {
    let mut state = state();
    assert!(state.active_text_input().is_none());
    open(&mut state, 0);
    assert!(
        state.active_text_input().is_some(),
        "caret, IME and clipboard find it"
    );
    type_str(&mut state, "hi");
    assert_eq!(state.active_text_input().unwrap().text(), "hi");
    assert!(state.editor_ui.projects_panel.owns_keyboard());
}
