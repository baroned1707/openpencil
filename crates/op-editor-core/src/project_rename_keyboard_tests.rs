use super::*;
use crate::document_tabs::DocumentTabStatus;
use crate::project_rename::RenameNameError;
use crate::project_tree::{ProjectFileKind, ProjectFileView, ProjectView};

fn state_renaming(label: &str) -> EditorState {
    let mut state = EditorState::starter();
    let panel = &mut state.editor_ui.projects_panel;
    panel.projects = vec![ProjectView {
        root: "/p".to_string(),
        name: "p".to_string(),
        files: vec![ProjectFileView {
            path: format!("/p/{label}"),
            label: label.to_string(),
            open_id: None,
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: false,
            folder: String::new(),
            kind: ProjectFileKind::Design,
        }],
        ..ProjectView::default()
    }];
    assert!(panel.begin_rename(&format!("/p/{label}"), 1_000));
    state
}

fn draft(state: &EditorState) -> String {
    state
        .editor_ui
        .projects_panel
        .rename
        .as_ref()
        .unwrap()
        .input
        .text()
        .to_string()
}

#[test]
fn no_rename_means_no_key_is_owned() {
    let mut state = EditorState::starter();
    assert_eq!(text(&mut state, 'a', 0), None);
    assert_eq!(paste(&mut state, "a", 0), None);
    assert_eq!(backspace(&mut state, 0), None);
    assert_eq!(delete_forward(&mut state, 0), None);
    assert!(!move_caret(&mut state, true, false, 0));
    assert!(!move_caret_to_edge(&mut state, true, false, 0));
    assert!(!select_all(&mut state, 0));
    assert_eq!(confirm(&mut state), None);
    assert!(!cancel(&mut state));
    assert!(!is_active(&state));
}

#[test]
fn typing_replaces_the_selected_stem_and_control_chars_are_dropped() {
    let mut state = state_renaming("card.op");
    assert!(is_active(&state));
    assert_eq!(text(&mut state, 'h', 1_001), Some(true));
    assert_eq!(draft(&state), "h");
    assert_eq!(text(&mut state, '\n', 1_002), Some(false));
    assert_eq!(text(&mut state, '设', 1_003), Some(true));
    assert_eq!(draft(&state), "h设");
}

#[test]
fn a_forbidden_character_is_typed_and_flagged_live() {
    let mut state = state_renaming("card.op");
    text(&mut state, 'a', 1_001);
    text(&mut state, '/', 1_002);
    assert_eq!(draft(&state), "a/");
    assert_eq!(
        state
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap()
            .error,
        Some(RenameNameError::Separator)
    );
    assert_eq!(backspace(&mut state, 1_003), Some(true));
    assert_eq!(
        state
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap()
            .error,
        None,
        "the message clears with the cause"
    );
}

#[test]
fn paste_is_single_line() {
    let mut state = state_renaming("card.op");
    assert_eq!(paste(&mut state, "a\nb\tc", 1_001), Some(true));
    assert_eq!(draft(&state), "abc");
    assert_eq!(paste(&mut state, "\n\n", 1_002), Some(false));
}

#[test]
fn caret_keys_and_select_all_edit_in_place() {
    let mut state = state_renaming("card.op");
    assert!(move_caret_to_edge(&mut state, true, false, 1_001));
    assert!(move_caret(&mut state, false, false, 1_002));
    assert_eq!(delete_forward(&mut state, 1_003), Some(true));
    assert_eq!(draft(&state), "car");
    assert!(select_all(&mut state, 1_004));
    assert_eq!(selected_text(&state).as_deref(), Some("car"));
    assert_eq!(backspace(&mut state, 1_005), Some(true));
    assert_eq!(draft(&state), "");
}

#[test]
fn enter_confirms_and_escape_cancels() {
    let mut state = state_renaming("card.op");
    text(&mut state, 'x', 1_001);
    assert_eq!(confirm(&mut state), Some(RenameConfirm::Submitted));
    assert!(state.editor_ui.projects_panel.rename_commit.is_some());
    assert!(cancel(&mut state));
    assert!(!is_active(&state));
}
