//! MCP edits of an image's source clear the credit of the picture they replace.

use std::collections::BTreeMap;

use op_editor_core::{node_image_credit, walkers, EditorState, ImageCredit, NodeId};

use super::batch_design_snapshot;
use super::write_tools::update_node_snapshot;
use super::{McpTool, ToolOutcome};

const STOCK: &str = "data:image/png;base64,c3RvY2s=";
const LOCAL: &str = "data:image/png;base64,bG9jYWw=";

fn credited_state() -> EditorState {
    let document = serde_json::from_value(serde_json::json!({
        "version": "1.0.0", "children": [{
            "type": "image", "id": "photo", "width": 64, "height": 64, "src": STOCK,
            "imageCredit": {"provider": "openverse", "creator": "Jane Doe", "license": "CC BY 4.0"}
        }]
    }))
    .unwrap();
    EditorState::from_document(document)
}

fn credit_of(state: &EditorState) -> Option<ImageCredit> {
    walkers::find_node(state.active_children(), &NodeId::new("photo"))
        .and_then(|node| node_image_credit(node).cloned())
}

#[test]
fn update_node_src_change_clears_a_stale_credit() {
    let mut state = credited_state();
    assert!(credit_of(&state).is_some(), "fixture carries a credit");
    let mut args = BTreeMap::new();
    args.insert("node_id".into(), "photo".into());
    args.insert("data".into(), format!(r#"{{"src":"{LOCAL}"}}"#));
    let ToolOutcome::OkWithCommand(_, command) = update_node_snapshot().call(&args) else {
        panic!("update_node must emit a command");
    };
    assert!(state.apply(command));
    assert_eq!(credit_of(&state), None);
}

/// The command a `batch_design` call emitted, whichever envelope carries it.
fn command_of(state: &EditorState, operations: String) -> op_editor_core::EditorCommand {
    let mut args = BTreeMap::new();
    args.insert("operations".into(), operations);
    match batch_design_snapshot(state).call(&args) {
        ToolOutcome::OkJsonWithCommand(_, command) | ToolOutcome::OkWithCommand(_, command) => {
            command
        }
        other => panic!("U(...) must emit a command, got {other:?}"),
    }
}

#[test]
fn batch_design_update_of_src_clears_a_stale_credit() {
    let mut state = credited_state();
    let command = command_of(&state, format!(r#"U("photo", {{"src":"{LOCAL}"}})"#));
    assert!(state.apply(command));
    assert_eq!(credit_of(&state), None);

    // Renaming the node leaves its picture, and so its credit, alone.
    let mut state = credited_state();
    let command = command_of(&state, r#"U("photo", {"name":"Hero"})"#.into());
    assert!(state.apply(command));
    assert!(credit_of(&state).is_some());
}
