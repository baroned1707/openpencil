//! The stock-provider keys are secrets: no agent-facing surface (MCP tool
//! output, chat and agent system prompts, the editor-state `Debug` dump) may
//! carry them. `AgentSettings` is not `Serialize`, so nothing walks it into
//! JSON; this test pins that by setting sentinel keys and scanning the output
//! of every read-only MCP tool plus the prompt builders.

#![cfg(test)]

use std::path::Path;

use op_editor_core::EditorState;
use serde_json::{json, Value};

use super::*;

const PIXABAY_SENTINEL: &str = "PIXABAY-SENTINEL-7f3a91c2";
const UNSPLASH_SENTINEL: &str = "UNSPLASH-SENTINEL-b04d55e8";

fn state_with_keys() -> EditorState {
    let mut state = EditorState::new();
    let settings = &mut state.editor_ui.agent_settings;
    settings.pixabay_api_key = PIXABAY_SENTINEL.into();
    settings.unsplash_access_key = UNSPLASH_SENTINEL.into();
    settings.unsplash_enabled = true;
    state
}

fn assert_clean(label: &str, text: &str) {
    for sentinel in [PIXABAY_SENTINEL, UNSPLASH_SENTINEL] {
        assert!(
            !text.contains(sentinel),
            "{label} leaked a provider key: {}",
            &text[..text.len().min(200)]
        );
    }
}

fn rpc(state: &mut EditorState, path: &Path, method: &str, params: Value) -> String {
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
    process_message(state, path, &line)
        .expect("dispatch")
        .unwrap_or_default()
}

#[test]
fn keys_never_appear_in_agent_facing_state() {
    let mut state = state_with_keys();
    let path = std::env::temp_dir().join("keys-never-leak.op");

    // The editor-state Debug dump (panic messages, assert diffs, log lines).
    assert_clean(
        "AgentSettings Debug",
        &format!("{:?}", state.editor_ui.agent_settings),
    );

    // Every read-only MCP tool, called with no arguments (a refusal message
    // is output too), plus the handshake and the catalog.
    assert_clean(
        "initialize",
        &rpc(&mut state, &path, "initialize", json!({})),
    );
    let catalog = rpc(&mut state, &path, "tools/list", json!({}));
    assert_clean("tools/list", &catalog);
    let catalog: Value = serde_json::from_str(&catalog).expect("tools/list JSON");
    let read_only: Vec<String> = catalog["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter(|tool| tool["annotations"]["readOnlyHint"] == true)
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        read_only.len() >= 15,
        "the sweep must cover the read tools, found {read_only:?}"
    );
    for name in &read_only {
        let response = rpc(
            &mut state,
            &path,
            "tools/call",
            json!({"name": name, "arguments": {}}),
        );
        assert_clean(&format!("tool {name}"), &response);
    }
    for name in ["get_editor_state", "get_document_info"] {
        assert!(
            read_only.iter().any(|tool| tool == name),
            "{name} is part of the sweep"
        );
    }

    // The chat and agent system prompts and the design.md prompt that are
    // built from the editor state.
    assert_clean(
        "chat system prompt",
        &crate::chat_system_prompt::build_chat_system_prompt(&state, "add a hero image"),
    );
    assert_clean(
        "agent system prompt",
        &crate::chat_system_prompt::build_agent_system_prompt(&state),
    );
    assert_clean(
        "design.md prompt",
        &crate::design_md_llm::build_design_md_user_prompt(&state, "describe it"),
    );
}
