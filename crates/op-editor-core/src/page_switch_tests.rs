use super::*;
use crate::node_id::NodeId;

fn state_with_pages(count: usize) -> EditorState {
    let mut state = EditorState::default();
    while state.page_count() < count {
        state.add_page().expect("page added");
    }
    // `add_page` activates the page it adds; start from the first.
    state.ui.active_page_index = 0;
    state
}

fn select_something(state: &mut EditorState) {
    state.set_single_selection(NodeId::new("n1"));
    assert!(state.selection.anchor.is_real(), "fixture has a selection");
}

#[test]
fn a_single_page_document_lists_one_synthetic_page() {
    let state = EditorState::default();
    let summary = state.page_summary();
    assert_eq!(summary.names, ["Page 1"]);
    assert_eq!(summary.active, 0);
}

#[test]
fn the_summary_lists_pages_in_order_and_marks_the_active_one() {
    let mut state = state_with_pages(3);
    let names = state.page_names();
    assert_eq!(names.len(), 3);
    state.ui.active_page_index = 1;
    let summary = state.page_summary();
    assert_eq!(summary.names, names);
    assert_eq!(summary.active, 1);
}

#[test]
fn a_stale_active_index_is_clamped_in_the_summary() {
    let mut state = state_with_pages(2);
    state.ui.active_page_index = 9;
    assert_eq!(state.page_summary().active, 1);
}

#[test]
fn switching_to_another_page_activates_it_clears_the_selection_and_asks_for_a_refit() {
    let mut state = state_with_pages(3);
    select_something(&mut state);
    assert_eq!(switch_page(&mut state, 2), PageSwitch::Refit);
    assert_eq!(state.ui.active_page_index, 2);
    assert!(!state.selection.anchor.is_real());
}

#[test]
fn switching_to_the_active_page_clears_the_selection_but_keeps_the_view() {
    let mut state = state_with_pages(3);
    state.ui.active_page_index = 1;
    select_something(&mut state);
    assert_eq!(switch_page(&mut state, 1), PageSwitch::Preserved);
    assert_eq!(state.ui.active_page_index, 1);
    assert!(!state.selection.anchor.is_real());
}

#[test]
fn an_out_of_range_index_changes_no_page_and_asks_for_no_refit() {
    let mut state = state_with_pages(2);
    state.ui.active_page_index = 1;
    assert_eq!(switch_page(&mut state, 7), PageSwitch::Preserved);
    assert_eq!(state.ui.active_page_index, 1);
}
