use super::*;
use crate::document_tabs::DocumentTabStatus;
use crate::project_tree::{ProjectFileKind, ProjectFileView, ProjectView};

fn file(label: &str, removable: bool) -> ProjectFileView {
    ProjectFileView {
        path: format!("/p/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
        folder: String::new(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable,
    }
}

fn panel() -> ProjectsPanelState {
    ProjectsPanelState {
        projects: vec![ProjectView {
            root: "/p".to_string(),
            name: "p".to_string(),
            files: vec![file("a.op", true), file("b.op", false)],
            ..ProjectView::default()
        }],
        ..ProjectsPanelState::default()
    }
}

#[test]
fn confirming_hands_the_path_to_the_host_and_closes_the_question() {
    let mut panel = panel();
    assert!(panel.begin_trash_confirm("/p/a.op"));
    assert!(panel.is_trash_confirming("/p/a.op"));
    assert!(panel.trash_commit.is_none(), "asking removes nothing");
    assert!(panel.confirm_trash());
    assert_eq!(panel.trash_commit.as_deref(), Some("/p/a.op"));
    assert!(panel.trash_confirm.is_none());
    assert!(!panel.confirm_trash(), "nothing left to confirm");
}

#[test]
fn remove_confirmation_escape_cancels() {
    let mut panel = panel();
    panel.begin_trash_confirm("/p/a.op");
    assert!(panel.cancel_trash_confirm());
    assert!(panel.trash_confirm.is_none());
    assert!(panel.trash_commit.is_none(), "cancel never removes");
    assert!(!panel.cancel_trash_confirm(), "second Escape is not ours");
}

#[test]
fn only_removable_rows_can_ask() {
    let mut panel = panel();
    assert!(!panel.begin_trash_confirm("/p/b.op"), "host said no");
    assert!(!panel.begin_trash_confirm("/p/gone.op"), "no such row");
    assert!(panel.trash_confirm.is_none());
}

#[test]
fn a_rename_and_a_question_never_coexist() {
    let mut panel = panel();
    assert!(panel.begin_rename("/p/b.op", 0));
    assert!(panel.begin_trash_confirm("/p/a.op"));
    assert!(panel.rename.is_none(), "asking drops the rename");
    assert!(panel.begin_rename("/p/b.op", 0));
    assert!(panel.trash_confirm.is_none(), "renaming drops the question");
}

#[test]
fn the_question_goes_when_its_row_does() {
    let mut panel = panel();
    panel.begin_trash_confirm("/p/a.op");
    assert!(!panel.drop_hidden_trash_confirm(), "row still there");
    panel.projects[0].files.remove(0);
    assert!(panel.drop_hidden_trash_confirm());
    assert!(panel.trash_confirm.is_none());

    let mut panel = self::panel();
    panel.begin_trash_confirm("/p/a.op");
    panel.projects[0].expanded = false;
    assert!(panel.drop_hidden_trash_confirm(), "folded project hides it");
}
