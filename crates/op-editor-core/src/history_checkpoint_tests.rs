//! `EditorState::revert_to_snapshot_with_undo`: a revert is one undo step of
//! the same document, not a new document.

#![cfg(test)]

use crate::node_id::NodeId;
use crate::state::EditorState;
use crate::test_support::{rect, state_with};

fn ids(state: &EditorState) -> Vec<String> {
    state
        .doc
        .children
        .iter()
        .map(|node| crate::pen_node_ext::PenNodeExt::id_str(node).to_string())
        .collect()
}

/// A state that already has two undo steps, then a checkpoint, then more
/// edits (the "agent run").
fn state_after_a_run() -> (EditorState, crate::EditorSnapshot) {
    let mut state = state_with(vec![rect("a", "A", 0.0, 0.0, 10.0, 10.0)]);
    for index in 0..2 {
        state.commit_history();
        state
            .doc
            .children
            .push(rect(&format!("early{index}"), "E", 0.0, 0.0, 5.0, 5.0));
    }
    let checkpoint = state.snapshot_for_history();
    state.commit_history();
    state
        .doc
        .children
        .push(rect("agent", "G", 1.0, 1.0, 9.0, 9.0));
    state.doc.name = Some("agent name".into());
    (state, checkpoint)
}

#[test]
fn revert_restores_the_document_content_of_the_checkpoint() {
    let (mut state, checkpoint) = state_after_a_run();
    assert!(ids(&state).contains(&"agent".to_string()));

    state.revert_to_snapshot_with_undo(&checkpoint);

    assert_eq!(ids(&state), ["a", "early0", "early1"]);
    assert_eq!(state.doc.name, None);
}

#[test]
fn revert_is_one_undo_step_and_keeps_the_earlier_steps() {
    let (mut state, checkpoint) = state_after_a_run();
    let steps_before = state.history.past.len();

    state.revert_to_snapshot_with_undo(&checkpoint);
    assert_eq!(state.history.past.len(), steps_before + 1);

    assert!(state.undo(), "one undo takes the revert back");
    assert!(ids(&state).contains(&"agent".to_string()));
    assert_eq!(state.doc.name.as_deref(), Some("agent name"));
    assert_eq!(state.history.past.len(), steps_before);
    assert!(state.redo(), "and redo re-applies it");
    assert_eq!(ids(&state), ["a", "early0", "early1"]);
}

#[test]
fn revert_makes_the_document_dirty_with_a_fresh_revision_and_keeps_the_generation() {
    let (mut state, checkpoint) = state_after_a_run();
    state.mark_saved_revision();
    let saved = state.document_revision();
    let generation = state.document_generation();

    state.revert_to_snapshot_with_undo(&checkpoint);

    assert!(state.is_dirty());
    assert_ne!(state.document_revision(), saved);
    assert_ne!(state.document_revision(), checkpoint.revision);
    assert_eq!(state.document_generation(), generation);
}

#[test]
fn revert_clears_selection_and_stale_drafts_and_restores_the_active_page() {
    let (mut state, checkpoint) = state_after_a_run();
    state.set_single_selection(NodeId::new("agent"));
    state.ui.pen_in_progress = Some(NodeId::new("agent"));
    state.editor_ui.hovered_layer_id = Some(NodeId::new("agent"));
    state.ui.active_page_index = 3;

    state.revert_to_snapshot_with_undo(&checkpoint);

    assert!(state.selection.set.is_empty());
    assert!(state.ui.pen_in_progress.is_none());
    assert!(state.editor_ui.hovered_layer_id.is_none());
    assert_eq!(state.ui.active_page_index, checkpoint.active_page_index);
}

#[test]
fn a_checkpoint_shares_image_payloads_with_the_live_document() {
    let payload = format!("data:image/png;base64,{}", "A".repeat(200_000));
    let doc = jian_ops_schema::load_str(&format!(
        r#"{{"version":"1.0.0","children":[{{"type":"image","id":"img","src":"{payload}","width":10,"height":10}}]}}"#
    ))
    .expect("image document loads")
    .value;
    let mut state = EditorState::from_document(doc);
    let live_ptr = image_src_ptr(&state.doc.children[0]);

    let checkpoint = state.snapshot_for_history();
    let stored = checkpoint
        .doc
        .snapshot_find_node(0, &NodeId::new("img"))
        .expect("image node is in the checkpoint");
    assert_eq!(image_src_ptr(stored), live_ptr, "stored by Arc, not copied");

    state.commit_history();
    state.doc.name = Some("edited".into());
    state.revert_to_snapshot_with_undo(&checkpoint);
    assert_eq!(
        image_src_ptr(&state.doc.children[0]),
        live_ptr,
        "restoring does not copy the payload either"
    );
}

fn image_src_ptr(node: &jian_ops_schema::node::PenNode) -> *const u8 {
    match node {
        jian_ops_schema::node::PenNode::Image(image) => image.src.as_str().as_ptr(),
        other => panic!("not an image node: {other:?}"),
    }
}
