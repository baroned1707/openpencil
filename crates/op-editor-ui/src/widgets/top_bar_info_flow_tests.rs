use super::*;
use crate::widgets::press_flow::{apply_shared_top_bar_hit, TopBarPress};
use crate::widgets::TopBarHit;
use op_editor_core::{DocumentId, DocumentTabStatus, DocumentTabView};

fn tab(id: u64, dirty: bool, status: DocumentTabStatus, active: bool) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: format!("doc-{id}.op"),
        project: String::new(),
        dirty,
        status,
        active,
    }
}

fn state(tabs: Vec<DocumentTabView>) -> EditorState {
    let mut state = EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    state.editor_ui.document_tabs = tabs;
    state
}

#[test]
fn the_other_unsaved_chip_opens_the_switcher_on_the_first_other_unsaved_file() {
    let mut state = state(vec![
        tab(1, true, DocumentTabStatus::Idle, true),
        tab(2, false, DocumentTabStatus::Idle, false),
        tab(3, true, DocumentTabStatus::Idle, false),
    ]);
    state.editor_ui.file_menu_open = true;
    state.editor_ui.account_menu_open = true;
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::OtherUnsaved, 10);
    assert_eq!(outcome, TopBarPress::Handled);
    assert!(state.editor_ui.projects_panel.switcher.open);
    assert!(
        !state.editor_ui.file_menu_open,
        "no dropdown stays under the switcher"
    );
    assert!(!state.editor_ui.account_menu_open);
    let rows = op_editor_core::quick_switcher_keyboard::rows(&state);
    let selected = state.editor_ui.projects_panel.switcher.selected;
    assert_eq!(
        rows[selected].label, "doc-3.op",
        "not the active one, not the clean one"
    );
}

#[test]
fn the_other_unsaved_chip_does_nothing_where_there_is_no_switcher() {
    let mut state = state(vec![tab(1, true, DocumentTabStatus::Idle, true)]);
    state.editor_ui.document_tabs_supported = false;
    apply_shared_top_bar_hit(&mut state, TopBarHit::OtherUnsaved, 10);
    assert!(!state.editor_ui.projects_panel.switcher.open);
}

#[test]
fn the_agent_item_switches_to_the_document_with_the_run() {
    let mut state = state(vec![
        tab(1, false, DocumentTabStatus::Idle, true),
        tab(2, false, DocumentTabStatus::Idle, false),
        tab(3, false, DocumentTabStatus::Running, false),
    ]);
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::AgentActivity, 0);
    assert_eq!(outcome, TopBarPress::Handled);
    assert_eq!(
        state.editor_ui.pending_document_request,
        Some(DocumentTabRequest::Switch(DocumentId(3)))
    );
}

#[test]
fn a_stale_agent_item_asks_for_nothing() {
    // The run ended between the last frame and the press.
    let mut state = state(vec![
        tab(1, false, DocumentTabStatus::Idle, true),
        tab(3, false, DocumentTabStatus::Done, false),
    ]);
    apply_shared_top_bar_hit(&mut state, TopBarHit::AgentActivity, 0);
    assert_eq!(state.editor_ui.pending_document_request, None);
}

#[test]
fn the_save_marker_acts_on_nothing() {
    let mut state = state(vec![tab(1, true, DocumentTabStatus::Idle, true)]);
    let before = state.editor_ui.document_tabs.clone();
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::SaveState, 0);
    assert_eq!(outcome, TopBarPress::Handled);
    assert_eq!(state.editor_ui.pending_document_request, None);
    assert!(!state.editor_ui.projects_panel.switcher.open);
    assert_eq!(state.editor_ui.document_tabs, before);
}

fn state_on_file(path: Option<&str>) -> EditorState {
    use op_editor_core::{ProjectFileKind, ProjectFileView, ProjectView};
    let mut state = state(vec![tab(1, false, DocumentTabStatus::Idle, true)]);
    state.editor_ui.file_name_display = Some("hero.op".to_string());
    if let Some(path) = path {
        state.editor_ui.projects_panel.projects = vec![ProjectView {
            root: "/work/shop".to_string(),
            name: "shop".to_string(),
            files: vec![ProjectFileView {
                path: path.to_string(),
                label: "hero.op".to_string(),
                open_id: Some(DocumentId(1)),
                dirty: false,
                status: DocumentTabStatus::Idle,
                active: true,
                folder: String::new(),
                kind: ProjectFileKind::Design,
                missing: false,
                removable: true,
            }],
            ..ProjectView::default()
        }];
    }
    state
}

#[test]
fn the_breadcrumb_asks_the_dock_to_reveal_the_active_file_and_nothing_else() {
    let mut state = state_on_file(Some("/work/shop/hero.op"));
    let outcome = apply_shared_top_bar_hit(&mut state, TopBarHit::Breadcrumb, 0);
    assert_eq!(outcome, TopBarPress::Handled);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(op_editor_core::ProjectRequest::RevealFile(
            "/work/shop/hero.op".to_string()
        ))
    );
    // Reveal only: no rename starts, no document is asked for.
    assert!(state.editor_ui.projects_panel.rename.is_none());
    assert_eq!(state.editor_ui.pending_document_request, None);
}

#[test]
fn a_stale_breadcrumb_press_without_a_dock_row_asks_for_nothing() {
    let mut state = state_on_file(None);
    apply_shared_top_bar_hit(&mut state, TopBarHit::Breadcrumb, 0);
    assert_eq!(state.editor_ui.projects_panel.pending_request, None);
}
