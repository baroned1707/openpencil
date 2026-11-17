//! The catalog-level promises of the in-place save: it is NOT a new tool, the
//! schema no longer demands a `filePath`, and the server instructions that
//! tell an agent to use it still fit the client's cut-off.

use super::super::schemas::TOOL_SCHEMAS;
use super::super::server_instructions::{MAX_INSTRUCTIONS_CHARS, SERVER_INSTRUCTIONS};
use super::super::tool_catalog::LEAN_TOOLS;
use super::super::tool_profile::McpAccessProfile;
use super::super::tools_list::tools_list_response;
use op_editor_core::EditorState;

/// Invariant: extending `save_document` added no tool, so the full catalog
/// still has 144 static tools (+ the 37 kit tools) and the lean one six.
#[test]
fn tool_catalog_count_unchanged() {
    assert_eq!(TOOL_SCHEMAS.len(), 144);
    assert_eq!(LEAN_TOOLS.len(), 6);
    assert!(!LEAN_TOOLS.contains(&"save_document"));
    let state = EditorState::new();
    let listed: serde_json::Value = serde_json::from_str(&tools_list_response(
        "3",
        &state,
        false,
        McpAccessProfile::UNRESTRICTED,
    ))
    .expect("tools/list JSON");
    let tools = listed["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 144 + 37);
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["name"] == "save_document")
            .count(),
        1
    );
}

/// Invariant: `filePath` is optional (omitting it is the in-place form) and
/// the description says what each form does and what it refuses.
#[test]
fn save_document_schema_makes_file_path_optional_and_explains_both_forms() {
    let state = EditorState::new();
    let listed: serde_json::Value = serde_json::from_str(&tools_list_response(
        "3",
        &state,
        false,
        McpAccessProfile::UNRESTRICTED,
    ))
    .expect("tools/list JSON");
    let tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "save_document")
        .expect("save_document listed");
    assert!(tool["inputSchema"].get("required").is_none(), "{tool}");
    let description = tool["description"].as_str().unwrap();
    for needle in [
        "WITHOUT filePath",
        "its OWN file",
        "create_document",
        "changed on disk",
        "dirtyBefore",
        "WITH filePath",
    ] {
        assert!(description.contains(needle), "{needle}: {description}");
    }
}

/// Invariant: the instructions tell the agent to save when it finishes, word
/// it so it stays true for the lean catalog (which has no `save_document`), and
/// stay under Claude Code's 4,096-character cut with headroom.
#[test]
fn instructions_stay_under_limit() {
    let length = SERVER_INSTRUCTIONS.chars().count();
    assert!(length < MAX_INSTRUCTIONS_CHARS, "{length} chars");
    assert!(SERVER_INSTRUCTIONS.contains("When save_document is listed"));
    assert!(SERVER_INSTRUCTIONS.contains("call it without filePath"));
    assert!(SERVER_INSTRUCTIONS.contains("The editor may also save when your run ends"));
}
