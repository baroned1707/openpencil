//! Tests for the node-id pre-flight check.

use std::collections::BTreeMap;

use super::*;
use crate::test_fixtures::sample;
use crate::RequestId;
use op_editor_core::PenNodeExt;

fn call(tool: &str, key: &str, id: &str) -> ToolCall {
    let mut arguments = BTreeMap::new();
    arguments.insert(key.to_string(), id.to_string());
    ToolCall {
        id: RequestId::Num(1),
        tool: tool.to_string(),
        arguments,
    }
}

const NODE_WRITERS: &[&str] = &[
    "set_node_name",
    "set_node_fill_hex",
    "set_node_hidden",
    "set_ellipse_arc",
    "add_node_effect",
    "remove_node_effect",
    "update_node",
    "move_node",
    "delete_node",
    "replace_node",
    "copy_node",
];

/// Invariant: a node-writing call naming an id that exists nowhere is
/// answered with that id and the tools that find valid ones, not a dump.
#[test]
fn an_unknown_node_id_names_the_id_and_the_tools_that_find_valid_ids() {
    let state = sample();
    for tool in NODE_WRITERS {
        let message = unknown_node_refusal(&state, &call(tool, "node_id", "ghost"))
            .unwrap_or_else(|| panic!("{tool} must refuse an unknown id"));
        assert_eq!(
            message,
            "node 'ghost' not found — call read_nodes or find_node_by_name to get valid ids",
            "{tool}"
        );
    }
}

/// Invariant: the check never refuses a call that names a real node.
#[test]
fn an_existing_node_id_is_never_refused() {
    let state = sample();
    for tool in NODE_WRITERS {
        for id in ["n10", "n11", "n14"] {
            assert_eq!(
                unknown_node_refusal(&state, &call(tool, "node_id", id)),
                None,
                "{tool} {id}"
            );
        }
    }
}

/// Invariant: every spelling the tools accept for the id is checked.
#[test]
fn the_camel_case_and_source_id_spellings_are_checked_too() {
    let state = sample();
    assert!(unknown_node_refusal(&state, &call("update_node", "nodeId", "ghost")).is_some());
    assert!(unknown_node_refusal(&state, &call("copy_node", "sourceId", "ghost")).is_some());
    assert_eq!(
        unknown_node_refusal(&state, &call("update_node", "nodeId", "n11")),
        None
    );
}

/// Invariant: reads, other tools and an absent or blank id are left to the
/// tools themselves, whose own errors (and pinned behaviour) are unchanged.
#[test]
fn reads_other_tools_and_blank_ids_pass_through() {
    let state = sample();
    for tool in [
        "get_node",
        "get_node_children",
        "read_nodes",
        "batch_design",
    ] {
        assert_eq!(
            unknown_node_refusal(&state, &call(tool, "node_id", "ghost")),
            None,
            "{tool}"
        );
    }
    assert_eq!(
        unknown_node_refusal(&state, &call("set_node_name", "node_id", "  ")),
        None
    );
    let none = ToolCall {
        id: RequestId::Num(1),
        tool: "set_node_name".into(),
        arguments: BTreeMap::new(),
    };
    assert_eq!(unknown_node_refusal(&state, &none), None);
}

/// Invariant: an id with `__` may be a render-only component-instance child
/// anchor that is not in the tree, so it is never refused here.
#[test]
fn a_possible_instance_child_anchor_is_left_to_the_apply_step() {
    let state = sample();
    assert_eq!(
        unknown_node_refusal(&state, &call("set_node_name", "node_id", "inst__child")),
        None
    );
}

/// Invariant: the attribute writers act on the active page, so a node on
/// another page is refused with a page hint; the structural writers accept a
/// `pageId` and are not refused.
#[test]
fn a_node_on_another_page_is_refused_only_for_active_page_writers() {
    let mut state = sample();
    assert!(state.add_page().is_some(), "second page");
    let other_page_node = state.doc.pages.as_ref().expect("pages")[1].children[0]
        .id_str()
        .to_string();
    assert!(state.set_active_page(0));
    let message = unknown_node_refusal(&state, &call("set_node_name", "node_id", &other_page_node))
        .expect("another page is refused for an attribute writer");
    assert!(message.contains("another page"), "{message}");
    assert!(message.contains("set_active_page"), "{message}");
    assert_eq!(
        unknown_node_refusal(&state, &call("update_node", "node_id", &other_page_node)),
        None
    );
}
