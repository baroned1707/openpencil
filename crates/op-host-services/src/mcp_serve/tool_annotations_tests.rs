//! Tests for the `tools/list` annotations and Claude Code `_meta` keys.

use serde_json::Value;

use super::*;
use crate::mcp_serve::schemas::TOOL_SCHEMAS;
use crate::mcp_serve::tool_catalog::LEAN_TOOLS;
use crate::mcp_serve::tool_profile::{
    catalog_tool_names, McpAccessProfile, ToolAccess, TOOL_PROFILES,
};
use crate::mcp_serve::tools_list::tools_list_response;
use op_editor_core::EditorState;

fn listed_tools(profile: McpAccessProfile) -> Vec<Value> {
    let response = tools_list_response("1", &EditorState::new(), false, profile);
    let value: Value = serde_json::from_str(&response).expect("tools/list is valid JSON");
    value["result"]["tools"]
        .as_array()
        .expect("tools array")
        .clone()
}

fn find<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools
        .iter()
        .find(|tool| tool["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not listed"))
}

/// Invariant: no tool reaches a client without stating what it does to the
/// world, because a client that sees no hint assumes the worst (a write that
/// is destructive and open-world). `readOnlyHint` is `true` or absent.
#[test]
fn every_listed_tool_states_open_world_and_a_write_states_destructive() {
    for profile in [McpAccessProfile::UNRESTRICTED, McpAccessProfile::LEAN] {
        for tool in listed_tools(profile) {
            let annotations = &tool["annotations"];
            assert!(annotations["openWorldHint"].is_boolean(), "{tool}");
            if annotations["readOnlyHint"] == true {
                assert!(annotations["destructiveHint"].is_null(), "{tool}");
            } else {
                assert!(annotations["readOnlyHint"].is_null(), "{tool}");
                assert!(annotations["destructiveHint"].is_boolean(), "{tool}");
            }
        }
    }
}

/// Invariant: a tool that writes anything (even a file or a plan store, even
/// when the access table calls it `Read`) is never advertised read-only.
#[test]
fn read_only_hint_is_false_for_every_write_and_for_reads_that_write_elsewhere() {
    let tools = listed_tools(McpAccessProfile::UNRESTRICTED);
    for profile in TOOL_PROFILES {
        if is_debug_only(profile.name) {
            continue;
        }
        let hint = find(&tools, profile.name)["annotations"]["readOnlyHint"] == true;
        let expected = profile.access == ToolAccess::Read
            && !READ_BUT_WRITES_ELSEWHERE.contains(&profile.name);
        assert_eq!(hint, expected, "{}", profile.name);
    }
    for name in READ_BUT_WRITES_ELSEWHERE {
        assert_ne!(
            find(&tools, name)["annotations"]["readOnlyHint"],
            true,
            "{name}"
        );
    }
    for name in [
        "get_node",
        "read_nodes",
        "snapshot_layout",
        "get_screenshot",
    ] {
        assert_eq!(find(&tools, name)["annotations"]["readOnlyHint"], true);
    }
}

fn is_debug_only(name: &str) -> bool {
    crate::mcp_serve::tool_profile::is_debug_only_tool(name)
}

/// Invariant: every `Read` tool on a surface beyond the in-memory document
/// has been reviewed into exactly one list, so a new tool that writes a file
/// while the access table calls it `Read` cannot be advertised read-only by
/// omission.
#[test]
fn every_non_memory_read_tool_is_reviewed_into_one_of_the_two_lists() {
    for profile in TOOL_PROFILES {
        if profile.access != ToolAccess::Read || profile.surface == ToolSurface::InMemory {
            continue;
        }
        let writes = READ_BUT_WRITES_ELSEWHERE.contains(&profile.name);
        let reads = READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT.contains(&profile.name);
        assert!(
            writes ^ reads,
            "{} reads outside the in-memory document: list it in exactly one of \
             READ_BUT_WRITES_ELSEWHERE / READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT",
            profile.name
        );
        assert_eq!(hints_for(profile.name).read_only, reads, "{}", profile.name);
    }
}

/// Invariant: a tool whose own description says it can delete, replace or
/// regenerate nodes is advertised destructive, not merely the `delete_*` names.
#[test]
fn tools_that_can_delete_or_replace_nodes_through_their_arguments_are_destructive() {
    for name in [
        "batch_design",
        "design_skeleton",
        "design_content",
        "design_refine",
        "finalize_design",
        "update_generator_params",
        "apply_design_system",
        "run_design_agent",
    ] {
        assert!(hints_for(name).destructive, "{name}");
    }
    assert!(!hints_for("update_generator_params").idempotent);
}

/// Invariant: each by-name list names real classified tools, so a rename or
/// removal cannot leave a stale entry that silently stops applying.
#[test]
fn every_name_in_the_hint_lists_is_a_classified_tool() {
    let catalog = catalog_tool_names();
    for (list, names) in [
        ("READ_BUT_WRITES_ELSEWHERE", READ_BUT_WRITES_ELSEWHERE),
        (
            "READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT",
            READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT,
        ),
        ("DESTRUCTIVE_WRITES", DESTRUCTIVE_WRITES),
        ("IDEMPOTENT_WRITES", IDEMPOTENT_WRITES),
        ("IDEMPOTENT_FILE_WRITES", IDEMPOTENT_FILE_WRITES),
        ("LARGE_RESULT_TOOLS", LARGE_RESULT_TOOLS),
    ] {
        for name in names {
            assert!(
                profile_for(name).is_some(),
                "{list}: {name} is unclassified"
            );
            assert!(
                is_debug_only(name) || catalog.iter().any(|entry| entry == name),
                "{list}: {name} is not in the catalog"
            );
        }
    }
    for name in IDEMPOTENT_FILE_WRITES {
        assert!(
            READ_BUT_WRITES_ELSEWHERE.contains(name),
            "{name} is an idempotent file write, so it must also be a read-that-writes"
        );
    }
    for name in READ_BUT_WRITES_ELSEWHERE {
        assert_eq!(
            profile_for(name).map(|profile| profile.access),
            Some(ToolAccess::Read),
            "{name} is listed as a read-that-writes but the table says it is a write"
        );
    }
    for name in DESTRUCTIVE_WRITES.iter().chain(IDEMPOTENT_WRITES) {
        assert_eq!(
            profile_for(name).map(|profile| profile.access),
            Some(ToolAccess::Write),
            "{name} is listed as a write but the table says it is a read"
        );
    }
}

/// Invariant: a tool that deletes, removes, replaces or steps history is
/// advertised destructive, so adding `delete_x` without listing it fails.
#[test]
fn destructive_writes_cover_every_delete_remove_replace_and_history_tool() {
    for profile in TOOL_PROFILES {
        if profile.access != ToolAccess::Write {
            continue;
        }
        let name = profile.name;
        let by_name = ["delete_", "remove_", "replace_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
            || matches!(name, "undo" | "redo" | "cut_selected");
        assert!(
            hints_for(name).destructive || !by_name,
            "{name} deletes or replaces content but is not in DESTRUCTIVE_WRITES"
        );
    }
    assert!(hints_for("delete_node").destructive);
    assert!(hints_for("replace_node").destructive);
    assert!(hints_for("undo").destructive);
    assert!(!hints_for("insert_node").destructive);
    assert!(!hints_for("set_node_fill_hex").destructive);
}

#[test]
fn idempotent_hint_follows_the_set_rule_and_the_list() {
    assert!(hints_for("set_node_fill_hex").idempotent);
    assert!(hints_for("update_node").idempotent);
    assert!(hints_for("delete_node").idempotent);
    assert!(!hints_for("insert_node").idempotent);
    assert!(!hints_for("batch_design").idempotent);
    assert!(!hints_for("duplicate_selected").idempotent);
    assert!(!hints_for("add_page").idempotent);
}

/// Invariant: `openWorldHint` is true exactly for the tools that reach the
/// network (the table's `OutboundNetwork` surface), false for the local editor.
#[test]
fn open_world_hint_is_true_only_for_network_tools() {
    let tools = listed_tools(McpAccessProfile::UNRESTRICTED);
    let open: Vec<&str> = tools
        .iter()
        .filter(|tool| tool["annotations"]["openWorldHint"] == true)
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    let mut expected: Vec<&str> = TOOL_PROFILES
        .iter()
        .filter(|profile| profile.surface == ToolSurface::OutboundNetwork)
        .map(|profile| profile.name)
        .filter(|name| !is_debug_only(name))
        .collect();
    let mut open_sorted = open.clone();
    open_sorted.sort_unstable();
    expected.sort_unstable();
    assert_eq!(open_sorted, expected);
    assert!(open.contains(&"import_html_url"));
    assert!(!open.contains(&"import_web_snapshot"), "local file only");
    assert!(!open.contains(&"get_screenshot"));
}

/// Invariant: a per-document kit `insert_<component>` tool (not in the
/// table) is an additive, local write — never read-only.
#[test]
fn kit_insert_tools_are_additive_local_writes() {
    let tools = listed_tools(McpAccessProfile::UNRESTRICTED);
    let kit: Vec<&Value> = tools
        .iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("insert_") && profile_for(name).is_none())
        })
        .collect();
    assert!(
        !kit.is_empty(),
        "the empty document lists the built-in kits"
    );
    for tool in kit {
        assert_ne!(tool["annotations"]["readOnlyHint"], true, "{tool}");
        assert_eq!(tool["annotations"]["destructiveHint"], false, "{tool}");
        assert_eq!(tool["annotations"]["openWorldHint"], false, "{tool}");
    }
}

/// Invariant: the hints are added to the finished `tools/list`, never baked
/// into the `TOOL_SCHEMAS` constants the in-process design agent also uses.
#[test]
fn the_schema_constants_carry_no_hints() {
    for schema in TOOL_SCHEMAS {
        assert!(!schema.contains(r#""annotations""#), "{schema:.80}");
        assert!(!schema.contains(r#""_meta""#), "{schema:.80}");
    }
}

/// Invariant: exactly the six lean tools ask to load upfront, in both
/// catalogs; no other tool does.
#[test]
fn only_the_lean_tools_are_always_loaded() {
    for profile in [McpAccessProfile::UNRESTRICTED, McpAccessProfile::LEAN] {
        for tool in listed_tools(profile) {
            let name = tool["name"].as_str().expect("name");
            let always = tool["_meta"]["anthropic/alwaysLoad"] == true;
            assert_eq!(always, LEAN_TOOLS.contains(&name), "{name}");
        }
    }
}

/// Invariant: Claude Code cuts the description of an upfront-loaded tool at
/// 4,096 characters, so none of the always-loaded descriptions may exceed it.
#[test]
fn always_loaded_descriptions_fit_the_upfront_truncation_limit() {
    for profile in [McpAccessProfile::UNRESTRICTED, McpAccessProfile::LEAN] {
        let tools = listed_tools(profile);
        for name in LEAN_TOOLS {
            let length = find(&tools, name)["description"]
                .as_str()
                .expect("description")
                .chars()
                .count();
            assert!(length <= 4096, "{name} description is {length} chars");
        }
    }
}

/// Invariant: the large-result tools raise Claude Code's persist-to-file
/// threshold, within the documented 500,000-character ceiling.
#[test]
fn large_result_tools_raise_the_result_cap_within_the_ceiling() {
    let tools = listed_tools(McpAccessProfile::UNRESTRICTED);
    for name in LARGE_RESULT_TOOLS {
        let cap = find(&tools, name)["_meta"]["anthropic/maxResultSizeChars"]
            .as_u64()
            .unwrap_or_else(|| panic!("{name} has no maxResultSizeChars"));
        assert_eq!(cap, LARGE_RESULT_CHARS as u64);
        assert!(cap <= 500_000, "Claude Code's ceiling is 500,000");
    }
    assert!(find(&tools, "get_node")["_meta"].is_null());
}

#[test]
fn decorate_entry_leaves_a_non_object_untouched() {
    assert_eq!(decorate_entry("not json"), "not json");
    assert_eq!(
        decorate_entry(r#"{"description":"x"}"#),
        r#"{"description":"x"}"#
    );
}

/// Invariant: `save_document` writes a file (never read-only), saving the same
/// document twice leaves the same file (idempotent) and it replaces nothing the
/// editor does not own (not destructive): the hints an agent's client uses to
/// decide whether to ask the user first.
#[test]
fn save_document_is_advertised_as_a_non_destructive_idempotent_write() {
    let hints = hints_for("save_document");
    assert!(!hints.read_only);
    assert!(hints.idempotent);
    assert!(!hints.destructive);
    assert!(!hints.open_world);
    assert_eq!(
        annotations_json("save_document"),
        r#"{"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}"#
    );
}
