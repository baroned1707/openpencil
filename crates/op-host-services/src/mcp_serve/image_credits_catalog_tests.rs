//! Image credits added no tool and no instruction text: the catalog and the
//! agent-facing metadata are the same size as before.

use super::schemas::TOOL_SCHEMAS;
use super::server_instructions::{MAX_INSTRUCTIONS_CHARS, SERVER_INSTRUCTIONS};
use super::tool_catalog::LEAN_TOOLS;
use super::tool_profile::McpAccessProfile;
use super::tools_list::tools_list_response;
use op_editor_core::EditorState;

/// Invariant: credits are read through `get_document_info`, so the static
/// catalog stays at 144 tools and the lean set at its six named tools.
#[test]
fn tool_count_and_lean_set_unchanged() {
    assert_eq!(TOOL_SCHEMAS.len(), 144);
    assert_eq!(
        LEAN_TOOLS,
        [
            "get_editor_state",
            "get_guidelines",
            "batch_design",
            "snapshot_layout",
            "get_screenshot",
            "finalize_design",
        ]
    );
    let listed: serde_json::Value = serde_json::from_str(&tools_list_response(
        "3",
        &EditorState::new(),
        false,
        McpAccessProfile::UNRESTRICTED,
    ))
    .expect("tools/list JSON");
    let tools = listed["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 144 + 37);
}

/// Invariant: the one-sentence description change names the new field and
/// keeps the description lint (no `TS` prefix, no `TS compat`).
#[test]
fn get_document_info_description_mentions_credits() {
    let listed: serde_json::Value = serde_json::from_str(&tools_list_response(
        "3",
        &EditorState::new(),
        false,
        McpAccessProfile::UNRESTRICTED,
    ))
    .expect("tools/list JSON");
    let tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "get_document_info")
        .expect("get_document_info listed");
    let description = tool["description"].as_str().unwrap();
    assert!(description.contains("imageCredits"), "{description}");
    assert!(!description.starts_with("TS") && !description.contains("TS compat"));
}

/// Invariant: the server instructions were not extended for this feature.
#[test]
fn instructions_stay_under_limit() {
    let length = SERVER_INSTRUCTIONS.chars().count();
    assert!(length < MAX_INSTRUCTIONS_CHARS, "{length} chars");
    assert!(!SERVER_INSTRUCTIONS.contains("imageCredit"));
}
