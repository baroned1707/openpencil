//! Lint over every description an agent reads in `tools/list`: no
//! porting-history jargon, units stated on geometry tools, and overlapping
//! tools pointing at each other.

use serde_json::Value;

use crate::mcp_serve::tool_profile::McpAccessProfile;
use crate::mcp_serve::tools_list::tools_list_response;
use op_editor_core::EditorState;

fn listed(profile: McpAccessProfile) -> Vec<Value> {
    let response = tools_list_response("1", &EditorState::new(), false, profile);
    let value: Value = serde_json::from_str(&response).expect("valid JSON");
    value["result"]["tools"].as_array().expect("tools").clone()
}

fn description<'a>(tools: &'a [Value], name: &str) -> &'a str {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed"))["description"]
        .as_str()
        .expect("description")
}

/// Every `description` string in a tool entry, top level and per argument.
fn all_descriptions(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(object) => {
            for (key, value) in object {
                match value {
                    Value::String(text) if key == "description" => out.push(text.clone()),
                    _ => all_descriptions(value, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| all_descriptions(item, out)),
        _ => {}
    }
}

/// Invariant: a description never speaks in porting history ("TS-compatible",
/// "Accepted for TS compatibility", a `TS` word, "Rust MCP"). The agent has
/// never seen the old stack, so those words only cost it context and trust.
#[test]
fn no_description_carries_porting_history_jargon() {
    for profile in [McpAccessProfile::UNRESTRICTED, McpAccessProfile::LEAN] {
        for tool in listed(profile) {
            let mut texts = Vec::new();
            all_descriptions(&tool, &mut texts);
            for text in texts {
                let lower = text.to_lowercase();
                assert!(
                    !text.trim_start().starts_with("TS"),
                    "{}: {text}",
                    tool["name"]
                );
                for banned in ["ts compat", "ts-compat", "for ts ", "rust mcp", "rust flat"] {
                    assert!(!lower.contains(banned), "{}: {text}", tool["name"]);
                }
                let has_ts_word = text
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|word| word == "TS");
                assert!(!has_ts_word, "{}: {text}", tool["name"]);
            }
        }
    }
}

/// Invariant: a tool that takes or returns geometry says its unit.
#[test]
fn geometry_tools_state_that_lengths_are_document_px() {
    let tools = listed(McpAccessProfile::UNRESTRICTED);
    for name in [
        "insert_node",
        "update_node",
        "snapshot_layout",
        "find_empty_space",
        "get_canvas_bounds",
        "get_node",
        "get_node_children",
        "nudge_selected",
        "set_node_corner_radius",
    ] {
        let text = description(&tools, name);
        assert!(
            text.contains("document px") || text.contains("doc-px"),
            "{name} does not state its unit: {text}"
        );
    }
}

/// Invariant: tools that overlap name each other, so an agent picks the
/// right one instead of the first it sees.
#[test]
fn overlapping_tools_point_at_each_other() {
    let tools = listed(McpAccessProfile::UNRESTRICTED);
    for (from, to) in [
        ("set_node_fill_hex", "update_node"),
        ("update_node", "set_node_fill_hex"),
        ("insert_node", "batch_design"),
        ("snapshot_layout", "get_screenshot"),
        ("get_screenshot", "snapshot_layout"),
        ("export_item", "export_nodes"),
        ("export_nodes", "export_item"),
        ("get_variables", "list_variables"),
        ("list_variables", "get_variables"),
        ("save_document", "create_document"),
    ] {
        assert!(
            description(&tools, from).contains(to),
            "{from} should point at {to}: {}",
            description(&tools, from)
        );
    }
}

/// Invariant: `open_document` says what it does (reads metadata, never opens
/// a tab) and `list_documents` no longer claims only the active document is
/// editable (parked documents are reachable through the host's lens).
#[test]
fn document_tool_descriptions_match_what_the_tools_do() {
    let tools = listed(McpAccessProfile::UNRESTRICTED);
    let open = description(&tools, "open_document");
    assert!(open.contains("does NOT open an editor tab"), "{open}");
    assert!(open.contains("metadata"), "{open}");
    let list = description(&tools, "list_documents");
    assert!(!list.contains("Only the active document"), "{list}");
    assert!(
        list.contains("not on screen can be read and edited"),
        "{list}"
    );
}

/// Invariant: the lean catalog's screenshot description and schema agree on
/// `scale` (inline screenshots now take 0.25 to 2.0).
#[test]
fn screenshot_scale_range_is_documented_in_both_catalogs() {
    for profile in [McpAccessProfile::UNRESTRICTED, McpAccessProfile::LEAN] {
        let tools = listed(profile);
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == "get_screenshot")
            .expect("get_screenshot");
        let scale = &tool["inputSchema"]["properties"]["scale"];
        assert_eq!(scale["type"], "number");
        assert!(
            scale["description"].as_str().unwrap_or("").contains("0.25"),
            "{scale}"
        );
    }
}
