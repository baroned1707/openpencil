//! `spawn_agents` outside the editor's built-in design agent: the MCP funnel
//! (`process_message_with_applier_profiled`, shared by headless `--mcp`, the
//! live server in `mcp_live/live_call.rs` and ACP agents) must refuse a valid
//! call with a tool error instead of acking work nothing performs.

use op_editor_core::EditorState;
use serde_json::{json, Value};

use super::tool_catalog::LEAN_TOOLS;
use super::tool_profile::McpAccessProfile;
use super::{
    process_message_with_applier, process_message_with_applier_profiled, tool_text,
    tools_list_response, TOOL_SCHEMAS,
};
use op_mcp::spawn_agents_tool::SPAWN_AGENTS_UNSUPPORTED;

const VALID_CONFIG: &str = r#"[{"prompt":"Fill the hero","containerNodes":["n1"],"styleguideName":"brand","guidelineNames":["web-app"]}]"#;

fn spawn_line(config: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {"name": "spawn_agents", "arguments": {"config": config}},
    })
    .to_string()
}

fn no_apply(_: &str, _: &mut EditorState, _: &op_editor_core::EditorCommand) -> bool {
    panic!("spawn_agents must never apply a command");
}

fn is_error(response: &str) -> bool {
    response.contains(r#""isError":true"#)
}

/// Invariant: headless `--mcp` gets the same honest refusal as every other
/// MCP connection — an `isError` result carrying `[ToolFailed]` and the shared
/// text, and no `spawned` / `agentIds` success payload.
#[test]
fn headless_spawn_agents_returns_the_same_honest_error() {
    let mut state = EditorState::new();
    let response = process_message_with_applier(&mut state, &spawn_line(VALID_CONFIG), no_apply)
        .expect("dispatch")
        .expect("response");
    assert!(is_error(&response), "{response}");
    let text = tool_text(&response);
    assert_eq!(
        text,
        format!("Error: [ToolFailed] {SPAWN_AGENTS_UNSUPPORTED}")
    );
    assert!(!response.contains("agentIds") && !response.contains("spawned"));

    // The live server calls the same funnel with its connection profile.
    let mut state = EditorState::new();
    let live = process_message_with_applier_profiled(
        &mut state,
        &spawn_line(VALID_CONFIG),
        McpAccessProfile::UNRESTRICTED,
        no_apply,
    )
    .expect("dispatch")
    .expect("response");
    assert_eq!(tool_text(&live), text);
    assert!(is_error(&live));
}

/// Invariant: a malformed call still reports its own argument error first.
#[test]
fn mcp_spawn_agents_validation_runs_before_the_refusal() {
    let mut state = EditorState::new();
    let response = process_message_with_applier(
        &mut state,
        &spawn_line(r#"[{"prompt":"Hero","styleguideName":""}]"#),
        no_apply,
    )
    .expect("dispatch")
    .expect("response");
    assert!(is_error(&response), "{response}");
    let text = tool_text(&response);
    assert!(text.starts_with("Error: [InvalidArgument]"), "{text}");
    assert!(text.contains("styleguideName must be non-empty"), "{text}");
    assert!(!text.contains("built-in design agent"), "{text}");
}

/// Invariant: refusing the call does not shrink or reorder any catalog — the
/// full catalog keeps its 144 tools (spawn_agents among them), the lean catalog
/// its six, and the in-app design toolset its 16.
#[test]
fn tool_count_and_lean_set_unchanged() {
    assert_eq!(TOOL_SCHEMAS.len(), 144);
    assert!(TOOL_SCHEMAS
        .iter()
        .any(|schema| schema.contains(r#""name":"spawn_agents""#)));
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
    let response = tools_list_response("1", &EditorState::new(), false, McpAccessProfile::LEAN);
    let value: Value = serde_json::from_str(&response).expect("tools/list JSON");
    assert_eq!(value["result"]["tools"].as_array().expect("tools").len(), 6);
    assert_eq!(crate::design_agent_tools::design_tool_defs().len(), 16);
}
