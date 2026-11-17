//! Pre-flight check of the node id a node-writing tool call names.
//!
//! A write tool only builds a command; whether the id exists is decided when
//! the host applies it, and a refused apply used to reach the agent as
//! `host rejected command: SetNodeName { node_id: NodeId("...") ... }`. That
//! tells the agent nothing it can act on. This check runs against the same
//! snapshot the tool would, BEFORE dispatch, at the one place every
//! transport (stdio, HTTP, live editor, per-file) funnels through, and says
//! which id was wrong and how to find a valid one.
//!
//! It never rejects a call that could succeed:
//!
//! - the attribute writers (`set_node_*`, effects) are applied to the ACTIVE
//!   page only, so for them a node that exists on another page is refused,
//!   with the page hint;
//! - the structural writers (`update_node`, `move_node`, ...) may carry a
//!   `pageId`, so for them an id is refused only when it exists on no page;
//! - an id containing `__` may be a render-only component-instance child
//!   anchor, which is not in the document tree, so it always passes.

use op_editor_core::{conversion::node_exists, walkers, EditorState, NodeId};

use super::ToolCall;

/// Structural writers: the node may be on any page.
const ANY_PAGE_WRITERS: &[&str] = &[
    "update_node",
    "move_node",
    "delete_node",
    "replace_node",
    "copy_node",
];

/// Attribute writers that are applied to the active page only.
fn is_active_page_writer(tool: &str) -> bool {
    tool.starts_with("set_node_")
        || matches!(
            tool,
            "set_ellipse_arc" | "add_node_effect" | "remove_node_effect"
        )
}

/// The argument spellings that carry the target node id, in the order the
/// tools themselves prefer them.
const ID_KEYS: &[&str] = &["node_id", "nodeId", "sourceId"];

/// The refusal message for `call` when it names a node that is not there,
/// else `None`. Reads and unknown tools are never refused here.
pub fn unknown_node_refusal(state: &EditorState, call: &ToolCall) -> Option<String> {
    let active_only = is_active_page_writer(&call.tool);
    if !active_only && !ANY_PAGE_WRITERS.contains(&call.tool.as_str()) {
        return None;
    }
    let raw = ID_KEYS
        .iter()
        .find_map(|key| call.arguments.get(*key))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())?;
    if raw.contains("__") {
        return None;
    }
    if !node_exists(&state.doc, raw) {
        return Some(format!(
            "node '{raw}' not found — call read_nodes or find_node_by_name to get valid ids"
        ));
    }
    if active_only && walkers::find_node(state.active_children(), &NodeId::new(raw)).is_none() {
        return Some(format!(
            "node '{raw}' is on another page, not the active page — call set_active_page \
             for its page first, then retry"
        ));
    }
    None
}

#[cfg(test)]
#[path = "node_id_check_tests.rs"]
mod tests;
