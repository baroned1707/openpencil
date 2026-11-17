use super::*;
use crate::document_tabs::DocumentTabStatus;
use crate::project_tree::ProjectFileKind;

fn file(folder: &str, label: &str) -> ProjectFileView {
    ProjectFileView {
        path: format!("/p/{folder}/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
        folder: folder.to_string(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

fn panel(files: Vec<ProjectFileView>) -> ProjectsPanelState {
    let mut panel = ProjectsPanelState::default();
    panel.projects = vec![ProjectView {
        root: "/p".to_string(),
        name: "p".to_string(),
        files,
        ..ProjectView::default()
    }];
    panel
}

#[test]
fn invalid_names_are_rejected_by_rule() {
    let long = "a".repeat(200);
    let table: [(&str, RenameNameError); 12] = [
        ("", RenameNameError::Empty),
        ("   ", RenameNameError::Empty),
        (".op", RenameNameError::Empty),
        ("..", RenameNameError::DotName),
        (".", RenameNameError::DotName),
        ("a/b", RenameNameError::Separator),
        ("a\\b", RenameNameError::Separator),
        (".hidden", RenameNameError::LeadingDot),
        ("a\0b", RenameNameError::Control),
        (long.as_str(), RenameNameError::TooLong),
        ("trailing.", RenameNameError::TrailingDotOrSpace),
        ("a:b", RenameNameError::ReservedChar),
    ];
    for (draft, expected) in table {
        assert_eq!(normalized_stem(draft), Err(expected), "{draft:?}");
    }
}

#[test]
fn a_name_is_trimmed_and_a_typed_extension_is_dropped() {
    assert_eq!(normalized_stem("  card  ").as_deref(), Ok("card"));
    assert_eq!(normalized_stem("card.op").as_deref(), Ok("card"));
    assert_eq!(normalized_stem("Card.OP").as_deref(), Ok("Card"));
    assert_eq!(normalized_stem("card.op.op").as_deref(), Ok("card.op"));
    assert_eq!(normalized_stem("v1.2 final").as_deref(), Ok("v1.2 final"));
    assert_eq!(normalized_stem("星图 首页").as_deref(), Ok("星图 首页"));
}

#[test]
fn the_length_cap_counts_bytes_of_the_stem() {
    assert!(normalized_stem(&"a".repeat(MAX_STEM_BYTES)).is_ok());
    assert_eq!(
        normalized_stem(&"a".repeat(MAX_STEM_BYTES + 1)),
        Err(RenameNameError::TooLong)
    );
    // 41 three-byte characters are 123 bytes: over the cap although short.
    assert_eq!(
        normalized_stem(&"设".repeat(41)),
        Err(RenameNameError::TooLong)
    );
}

#[test]
fn rename_starts_on_an_op_row_with_the_stem_selected() {
    let mut panel = panel(vec![file("", "card.op")]);
    assert!(panel.begin_rename("/p//card.op", 10));
    let edit = panel.rename.as_ref().unwrap();
    assert_eq!(edit.input.text(), "card");
    assert_eq!(edit.input.highlight_range(), Some((0, 4)), "stem selected");
    assert_eq!(edit.error, None);
}

#[test]
fn fig_rows_and_other_extensions_are_not_renamable() {
    let mut fig = file("", "brand.fig");
    fig.kind = ProjectFileKind::Figma(crate::project_tree::FigStatus::NotConverted);
    let mut panel = panel(vec![fig, file("", "legacy.pen"), file("", "ok.op")]);
    assert!(!panel.begin_rename("/p//brand.fig", 0));
    assert!(!panel.begin_rename("/p//legacy.pen", 0));
    assert!(!panel.begin_rename("/p//missing.op", 0));
    assert!(panel.rename.is_none());
    assert!(panel.begin_rename("/p//ok.op", 0));
}

#[test]
fn confirming_a_new_name_records_a_request_and_keeps_the_edit_open() {
    let mut panel = panel(vec![file("", "card.op")]);
    panel.begin_rename("/p//card.op", 0);
    panel.rename.as_mut().unwrap().input.set_text("hero");
    assert_eq!(panel.confirm_rename(), RenameConfirm::Submitted);
    assert_eq!(
        panel.rename_commit,
        Some(ProjectRenameRequest {
            path: "/p//card.op".to_string(),
            new_stem: "hero".to_string()
        })
    );
    assert!(panel.rename.is_some(), "open until the host answers");
    panel.finish_rename("/p//card.op");
    assert!(panel.rename.is_none());
}

#[test]
fn an_unchanged_name_closes_silently() {
    let mut panel = panel(vec![file("", "card.op")]);
    panel.begin_rename("/p//card.op", 0);
    assert_eq!(panel.confirm_rename(), RenameConfirm::Unchanged);
    assert!(panel.rename.is_none());
    assert!(panel.rename_commit.is_none());
}

#[test]
fn a_case_only_change_is_a_real_rename() {
    let mut panel = panel(vec![file("", "Card.op")]);
    panel.begin_rename("/p//Card.op", 0);
    panel.rename.as_mut().unwrap().input.set_text("card");
    assert_eq!(panel.confirm_rename(), RenameConfirm::Submitted);
}

#[test]
fn a_sibling_with_the_same_name_ignoring_case_is_a_collision() {
    let mut panel = panel(vec![
        file("a", "card.op"),
        file("a", "Hero.op"),
        file("b", "hero.op"),
    ]);
    panel.begin_rename("/p/a/card.op", 0);
    panel.rename.as_mut().unwrap().input.set_text("hero");
    panel.refresh_rename_error();
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        Some(RenameNameError::Taken),
        "live"
    );
    assert_eq!(panel.confirm_rename(), RenameConfirm::Rejected);
    assert!(panel.rename_commit.is_none());
    // The same name in ANOTHER folder is no collision.
    panel.rename.as_mut().unwrap().input.set_text("hero2");
    panel.refresh_rename_error();
    assert_eq!(panel.rename.as_ref().unwrap().error, None);
}

#[test]
fn an_invalid_draft_keeps_the_edit_open_and_blank_waits_for_enter() {
    let mut panel = panel(vec![file("", "card.op")]);
    panel.begin_rename("/p//card.op", 0);
    panel.rename.as_mut().unwrap().input.set_text("a/b");
    panel.refresh_rename_error();
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        Some(RenameNameError::Separator)
    );
    panel.rename.as_mut().unwrap().input.set_text("");
    panel.refresh_rename_error();
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        None,
        "quiet while empty"
    );
    assert_eq!(panel.confirm_rename(), RenameConfirm::Rejected);
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        Some(RenameNameError::Empty)
    );
}

#[test]
fn click_away_commits_a_valid_name_and_cancels_any_other() {
    let mut valid = panel(vec![file("", "card.op")]);
    valid.begin_rename("/p//card.op", 0);
    valid.rename.as_mut().unwrap().input.set_text("hero");
    assert!(valid.commit_or_cancel_rename());
    assert!(valid.rename_commit.is_some());

    let mut invalid = panel(vec![file("", "card.op")]);
    invalid.begin_rename("/p//card.op", 0);
    invalid.rename.as_mut().unwrap().input.set_text("a/b");
    assert!(invalid.commit_or_cancel_rename());
    assert!(invalid.rename.is_none(), "cancelled");
    assert!(invalid.rename_commit.is_none());

    assert!(!invalid.commit_or_cancel_rename(), "nothing was open");
}

#[test]
fn the_host_can_reject_a_name_and_the_edit_stays_open() {
    let mut panel = panel(vec![file("", "card.op")]);
    panel.begin_rename("/p//card.op", 0);
    panel.reject_rename("/p//card.op", RenameNameError::Taken);
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        Some(RenameNameError::Taken)
    );
    // An answer about another file changes nothing.
    panel.reject_rename("/p//other.op", RenameNameError::Empty);
    assert_eq!(
        panel.rename.as_ref().unwrap().error,
        Some(RenameNameError::Taken)
    );
}

#[test]
fn an_edit_whose_row_is_hidden_is_dropped() {
    let mut panel = panel(vec![file("sub", "card.op")]);
    panel.begin_rename("/p/sub/card.op", 0);
    assert!(panel.rename_row_visible());
    assert!(!panel.drop_hidden_rename());
    panel.projects[0].collapsed_folders.push("sub".to_string());
    assert!(panel.drop_hidden_rename(), "folded folder");
    panel.projects[0].collapsed_folders.clear();
    panel.begin_rename("/p/sub/card.op", 0);
    panel.projects[0].files.clear();
    assert!(panel.drop_hidden_rename(), "file vanished");
    panel.projects[0].files = vec![file("sub", "card.op")];
    panel.begin_rename("/p/sub/card.op", 0);
    panel.projects[0].expanded = false;
    assert!(panel.drop_hidden_rename(), "project folded");
    panel.projects[0].expanded = true;
    panel.begin_rename("/p/sub/card.op", 0);
    panel.toggle_open();
    assert!(panel.drop_hidden_rename(), "dock closed");
}
