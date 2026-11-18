use super::*;
use crate::editor_ui_state::EditorUiState;
use crate::project_tree::{ProjectFileKind, ProjectFileView, ProjectView};
use crate::state::EditorState;
use crate::DocumentTabStatus;

fn row(label: &str, folder: &str, active: bool) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/{folder}/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: DocumentTabStatus::Idle,
        active,
        folder: folder.to_string(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: true,
    }
}

fn dock_ui(files: Vec<ProjectFileView>) -> EditorUiState {
    let mut ui = EditorUiState {
        document_tabs_supported: true,
        ..EditorUiState::default()
    };
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files,
        ..ProjectView::default()
    }];
    ui.file_name_display = Some("hero.op".to_string());
    ui
}

#[test]
fn names_project_folder_and_file_of_the_active_row() {
    let ui = dock_ui(vec![
        row("cart.op", "checkout", false),
        row("hero.op", "landing/v2", true),
    ]);
    let info = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!(info.project, "shop");
    assert_eq!(info.folder, "landing/v2");
    assert_eq!(info.file_name, "hero.op");
    assert_eq!(info.save, SaveState::Saved);
}

#[test]
fn a_file_in_the_project_root_has_an_empty_folder() {
    let ui = dock_ui(vec![row("hero.op", "", true)]);
    let info = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!((info.project.as_str(), info.folder.as_str()), ("shop", ""));
}

#[test]
fn the_save_state_follows_dirty_name_and_missing_row() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    ui.document_dirty = true;
    assert_eq!(
        ActiveFileInfo::from_ui(&ui, "Untitled").save,
        SaveState::Edited
    );

    ui.projects_panel.projects[0].files[0].missing = true;
    assert_eq!(
        ActiveFileInfo::from_ui(&ui, "Untitled").save,
        SaveState::Missing,
        "a vanished file outranks the dirty flag"
    );

    ui.file_name_display = None;
    let untitled = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!(untitled.save, SaveState::Untitled);
    assert_eq!(untitled.file_name, "Untitled");
}

#[test]
fn without_the_dock_only_the_name_and_state_remain() {
    let mut ui = dock_ui(vec![row("hero.op", "landing", true)]);
    ui.document_tabs_supported = false;
    ui.document_dirty = true;
    let info = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!((info.project.as_str(), info.folder.as_str()), ("", ""));
    assert_eq!(info.file_name, "hero.op");
    assert_eq!(info.save, SaveState::Edited);

    let mut touch = dock_ui(vec![row("hero.op", "landing", true)]);
    touch.touch = true;
    assert_eq!(ActiveFileInfo::from_ui(&touch, "Untitled").project, "");
}

#[test]
fn a_loose_document_has_no_project() {
    let mut ui = dock_ui(vec![row("other.op", "", false)]);
    ui.file_name_display = None;
    let info = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!(info.project, "");
    assert_eq!(info.save, SaveState::Untitled);
}

#[test]
fn the_git_branch_comes_from_the_git_panel() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    assert_eq!(ActiveFileInfo::from_ui(&ui, "Untitled").git_branch, None);
    ui.git_panel.branch = Some("main".to_string());
    assert_eq!(
        ActiveFileInfo::from_ui(&ui, "Untitled")
            .git_branch
            .as_deref(),
        Some("main")
    );
}

#[test]
fn only_unsaved_states_show_the_unsaved_mark() {
    let mut info = ActiveFileInfo::default();
    assert!(!info.shows_unsaved_mark());
    for save in [SaveState::Edited, SaveState::Untitled, SaveState::Missing] {
        info.save = save;
        assert!(info.shows_unsaved_mark(), "{save:?}");
    }
}

fn tab(id: u64, dirty: bool, status: DocumentTabStatus, active: bool) -> crate::DocumentTabView {
    crate::DocumentTabView {
        id: crate::DocumentId(id),
        label: format!("doc-{id}.op"),
        project: String::new(),
        dirty,
        status,
        active,
    }
}

#[test]
fn other_unsaved_counts_dirty_documents_except_the_active_one() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    ui.document_tabs = vec![
        tab(1, true, DocumentTabStatus::Idle, true),
        tab(2, true, DocumentTabStatus::Idle, false),
        tab(3, false, DocumentTabStatus::Idle, false),
        tab(4, true, DocumentTabStatus::Idle, false),
    ];
    assert_eq!(
        ActiveFileInfo::from_ui(&ui, "Untitled").other_unsaved,
        2,
        "the active document's own unsaved state is not counted"
    );
    ui.document_tabs[0].dirty = false;
    assert_eq!(ActiveFileInfo::from_ui(&ui, "Untitled").other_unsaved, 2);
    ui.document_tabs[1].dirty = false;
    ui.document_tabs[3].dirty = false;
    assert_eq!(ActiveFileInfo::from_ui(&ui, "Untitled").other_unsaved, 0);
}

#[test]
fn a_run_counts_only_in_another_document() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    ui.document_tabs = vec![
        tab(1, false, DocumentTabStatus::Running, true),
        tab(2, false, DocumentTabStatus::Idle, false),
    ];
    let own_run = ActiveFileInfo::from_ui(&ui, "Untitled").running_elsewhere;
    assert_eq!(own_run, RunningElsewhere::default(), "idle shows nothing");

    ui.document_tabs[1].status = DocumentTabStatus::Running;
    let elsewhere = ActiveFileInfo::from_ui(&ui, "Untitled").running_elsewhere;
    assert_eq!(elsewhere.count, 1);
    assert_eq!(
        elsewhere.first,
        Some((crate::DocumentId(2), "doc-2.op".to_string()))
    );
}

#[test]
fn several_runs_report_the_count_and_the_first_in_open_order() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    ui.document_tabs = vec![
        tab(1, false, DocumentTabStatus::Idle, true),
        tab(2, false, DocumentTabStatus::Running, false),
        tab(3, false, DocumentTabStatus::NeedsReview, false),
        tab(4, false, DocumentTabStatus::Running, false),
    ];
    let elsewhere = ActiveFileInfo::from_ui(&ui, "Untitled").running_elsewhere;
    assert_eq!(elsewhere.count, 2, "only Running counts, not NeedsReview");
    assert_eq!(
        elsewhere.first.map(|(id, _)| id),
        Some(crate::DocumentId(2))
    );
}

#[test]
fn without_the_dock_other_documents_are_not_reported() {
    let mut ui = dock_ui(vec![row("hero.op", "", true)]);
    ui.document_tabs = vec![
        tab(1, false, DocumentTabStatus::Idle, true),
        tab(2, true, DocumentTabStatus::Running, false),
    ];
    ui.document_tabs_supported = false;
    let info = ActiveFileInfo::from_ui(&ui, "Untitled");
    assert_eq!(info.other_unsaved, 0);
    assert_eq!(info.running_elsewhere, RunningElsewhere::default());
}

#[test]
fn the_page_chip_needs_two_pages_and_the_dock() {
    let mut state = EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    let one = ActiveFileInfo::from_ui(&state.editor_ui, "Untitled").with_pages_of(&state);
    assert_eq!(one.pages, None, "a single page has nothing to switch to");

    while state.page_count() < 3 {
        state.add_page().expect("page added");
    }
    state.ui.active_page_index = 1;
    let three = ActiveFileInfo::from_ui(&state.editor_ui, "Untitled").with_pages_of(&state);
    let pages = three.pages.expect("three pages offer the chip");
    assert_eq!((pages.names.len(), pages.active), (3, 1));

    // The web has no dock and no page chip in v1.
    state.editor_ui.document_tabs_supported = false;
    let web = ActiveFileInfo::from_ui(&state.editor_ui, "Untitled").with_pages_of(&state);
    assert_eq!(web.pages, None);
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.touch = true;
    let touch = ActiveFileInfo::from_ui(&state.editor_ui, "Untitled").with_pages_of(&state);
    assert_eq!(touch.pages, None);
}

#[test]
fn the_file_path_is_known_only_for_a_file_the_dock_lists() {
    let ui = dock_ui(vec![row("hero.op", "landing", true)]);
    assert_eq!(
        ActiveFileInfo::from_ui(&ui, "Untitled")
            .file_path
            .as_deref(),
        Some("/work/shop/landing/hero.op")
    );
    let mut loose = dock_ui(vec![row("other.op", "", false)]);
    loose.file_name_display = None;
    assert_eq!(ActiveFileInfo::from_ui(&loose, "Untitled").file_path, None);
    let mut web = dock_ui(vec![row("hero.op", "", true)]);
    web.document_tabs_supported = false;
    assert_eq!(ActiveFileInfo::from_ui(&web, "Untitled").file_path, None);
}
