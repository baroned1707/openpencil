use super::*;
use crate::editor_ui_state::{Locale, ThemeMode};
use crate::NodeId;

fn state_with_selection(id: &str) -> EditorState {
    let mut state = EditorState::starter();
    state.selection.set = vec![NodeId::new(id)];
    state.selection.anchor = NodeId::new(id);
    state
}

/// Swap the live state with a fresh document and return the parked scope of
/// what was on screen before.
fn park_live_document(live: &mut EditorState, incoming: EditorState) -> DocumentScope {
    let mut scope = DocumentScope::from_state(incoming);
    live.exchange_document_scope(&mut scope);
    scope
}

#[test]
fn swap_back_restores_selection_viewport_history() {
    let mut live = state_with_selection("n10");
    live.viewport.pan(40.0, 25.0);
    live.mark_document_changed();
    let snapshot = live.snapshot_for_history();
    live.history_push_past(snapshot);
    let viewport = live.viewport;
    let revision = live.document_revision();

    let mut parked_a = park_live_document(&mut live, EditorState::starter());
    // B is now on screen and knows nothing about A.
    assert!(live.selection.is_empty());
    assert_eq!(live.viewport, Viewport::IDENTITY);
    assert!(!live.history.can_undo());
    assert_eq!(live.document_revision(), 0);

    live.exchange_document_scope(&mut parked_a);
    assert_eq!(live.selection.anchor, NodeId::new("n10"));
    assert_eq!(live.viewport, viewport);
    assert!(live.history.can_undo());
    assert_eq!(live.document_revision(), revision);
}

#[test]
fn app_level_fields_do_not_follow_a_swap() {
    let mut live = EditorState::starter();
    live.editor_ui.locale = Locale::Ja;
    live.editor_ui.theme_mode = ThemeMode::Light;
    live.editor_ui.touch = true;
    live.editor_ui.home.brand.available = true;
    live.editor_ui.sidebar_open = false;
    live.clipboard = live.doc.children.clone();

    let mut parked = park_live_document(&mut live, EditorState::starter());
    assert_eq!(live.editor_ui.locale, Locale::Ja);
    assert_eq!(live.editor_ui.theme_mode, ThemeMode::Light);
    assert!(live.editor_ui.touch);
    assert!(live.editor_ui.home.brand.available);
    assert!(!live.editor_ui.sidebar_open);
    assert_eq!(live.clipboard.len(), 1, "the clipboard crosses documents");

    live.exchange_document_scope(&mut parked);
    assert_eq!(live.editor_ui.locale, Locale::Ja);
    assert!(live.editor_ui.home.brand.available);
}

#[test]
fn document_owned_ui_fields_travel_with_their_document() {
    let mut live = EditorState::starter();
    live.editor_ui.file_name_display = Some("a.op".to_owned());
    live.editor_ui.collapsed_layers.insert(NodeId::new("n10"));
    live.editor_ui.home.recipe = None;
    live.editor_ui.home.imported_from = Some("https://example.com/".to_owned());
    live.editor_ui.pinned_style_guide = Some("brutalist".to_owned());

    let mut parked = park_live_document(&mut live, EditorState::starter());
    assert_eq!(live.editor_ui.file_name_display, None);
    assert!(live.editor_ui.collapsed_layers.is_empty());
    assert_eq!(live.editor_ui.home.imported_from, None);
    assert_eq!(live.editor_ui.pinned_style_guide, None);

    live.exchange_document_scope(&mut parked);
    assert_eq!(live.editor_ui.file_name_display.as_deref(), Some("a.op"));
    assert!(live
        .editor_ui
        .collapsed_layers
        .contains(&NodeId::new("n10")));
    assert_eq!(
        live.editor_ui.home.imported_from.as_deref(),
        Some("https://example.com/")
    );
    assert_eq!(
        live.editor_ui.pinned_style_guide.as_deref(),
        Some("brutalist")
    );
}

#[test]
fn dirty_flag_is_recomputed_for_the_incoming_document() {
    let mut live = EditorState::starter();
    live.mark_document_changed();
    assert!(live.editor_ui.document_dirty);

    let mut parked = park_live_document(&mut live, EditorState::starter());
    assert!(!live.is_dirty());
    assert!(!live.editor_ui.document_dirty, "clean document on screen");
    assert!(parked.is_dirty(), "the parked document keeps its edits");

    live.exchange_document_scope(&mut parked);
    assert!(live.editor_ui.document_dirty);
}

#[test]
fn parked_scope_accepts_a_save_ack_only_for_its_own_generation() {
    let mut live = EditorState::starter();
    live.mark_document_changed();
    let (generation, revision) = (live.document_generation(), live.document_revision());
    let mut parked = park_live_document(&mut live, EditorState::starter());

    assert!(!parked.mark_saved_revision_at(generation + 1, revision));
    assert!(parked.is_dirty());
    assert!(parked.mark_saved_revision_at(generation, revision));
    assert!(!parked.is_dirty());
    assert!(!live.is_dirty(), "the live document was never touched");
}

#[test]
fn a_fresh_scope_is_pristine_until_it_is_edited() {
    let mut state = EditorState::starter();
    assert!(DocumentScope::from_state(state.clone()).is_pristine());
    state.mark_document_changed();
    assert!(!DocumentScope::from_state(state).is_pristine());
}

#[test]
fn the_projects_dock_stays_as_it_was_across_a_document_switch() {
    let mut live = EditorState::starter();
    live.editor_ui.projects_panel.dock_open = false;
    live.editor_ui.projects_panel.scroll.offset = 90.0;

    // Picking a file in the dock switches documents; the dock is the
    // window's, not the document's, so it must come out the other side
    // exactly as the user left it — folded, and at the same scroll.
    let mut parked = park_live_document(&mut live, EditorState::starter());
    assert!(!live.editor_ui.projects_panel.dock_open);
    assert_eq!(live.editor_ui.projects_panel.scroll.offset, 90.0);

    live.exchange_document_scope(&mut parked);
    assert!(!live.editor_ui.projects_panel.dock_open);
    assert_eq!(live.editor_ui.projects_panel.scroll.offset, 90.0);
}

#[test]
fn another_documents_tab_still_travels_with_it() {
    use crate::editor_ui_state::LeftPanelTab;
    let mut live = EditorState::starter();
    live.editor_ui.slides_panel.tab = LeftPanelTab::Slides;
    let _parked = park_live_document(&mut live, EditorState::starter());
    assert_eq!(live.editor_ui.slides_panel.tab, LeftPanelTab::Layers);
}
