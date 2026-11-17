//! Tests for the errors an agent reads: unknown node ids, the `[Code]`
//! prefix, and the structured arguments the schemas advertise.

use serde_json::{json, Value};

use super::*;

fn call_line(tool: &str, arguments: Value) -> String {
    json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":tool,"arguments":arguments}})
        .to_string()
}

fn run(state: &mut op_editor_core::EditorState, line: &str, applied: &mut bool) -> String {
    process_message_with_applier(state, line, |_, _, _| {
        *applied = true;
        true
    })
    .expect("dispatch")
    .expect("response")
}

fn state_with_one_rect() -> (op_editor_core::EditorState, String) {
    let mut state = op_editor_core::EditorState::new();
    assert!(state.apply(EditorCommand::InsertNode {
        kind: "rect".into(),
        name: "Box".into(),
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        fill_hex: None,
        target_parent: op_editor_core::NodeId::NONE,
        page_id: None,
    }));
    let id = op_editor_core::PenNodeExt::id_str(&state.active_children()[0]).to_string();
    (state, id)
}

/// Invariant: a node-writing call naming an unknown node never reaches the
/// host; the agent gets the id and the tools that find valid ones, led by the
/// error code, instead of `host rejected command: {Debug dump}`.
#[test]
fn unknown_node_id_on_a_write_tool_is_refused_before_dispatch_with_a_fix_hint() {
    let (mut state, _) = state_with_one_rect();
    for (tool, arguments) in [
        ("set_node_name", json!({"node_id":"ghost","name":"x"})),
        ("set_node_fill_hex", json!({"node_id":"ghost","hex":"#fff"})),
        ("update_node", json!({"nodeId":"ghost","data":{"name":"x"}})),
        (
            "move_node",
            json!({"node_id":"ghost","target_parent_id":"x"}),
        ),
        ("delete_node", json!({"node_id":"ghost"})),
    ] {
        let mut applied = false;
        let response = run(&mut state, &call_line(tool, arguments), &mut applied);
        let value: Value = serde_json::from_str(&response).expect("json");
        assert_eq!(value["result"]["isError"], true, "{tool}: {response}");
        assert_eq!(
            value["result"]["content"][0]["text"],
            "Error: [InvalidArgument] node 'ghost' not found — call read_nodes or \
             find_node_by_name to get valid ids",
            "{tool}"
        );
        assert!(!applied, "{tool} must not reach the applier");
        assert!(!response.contains("host rejected"), "{tool}: {response}");
    }
}

/// Invariant: the pre-check does not get in the way of a real node.
#[test]
fn a_known_node_id_still_reaches_the_applier() {
    let (mut state, id) = state_with_one_rect();
    let mut applied = false;
    let line = call_line("set_node_name", json!({"node_id": id, "name": "Renamed"}));
    let response = run(&mut state, &line, &mut applied);
    assert!(!response.contains(r#""isError""#), "{response}");
    assert!(applied);
}

/// Invariant: a profile refusal is a tool error with a code prefix too, and
/// keeps its own machine-readable lead after it.
#[test]
fn a_profile_refusal_leads_with_the_error_code_and_keeps_its_own_code() {
    let mut state = op_editor_core::EditorState::new();
    let line = call_line("get_document_info", json!({}));
    let response = process_message_with_applier_profiled(
        &mut state,
        &line,
        tool_profile::McpAccessProfile::LEAN,
        |_, _, _| true,
    )
    .expect("dispatch")
    .expect("response");
    let text = tool_text(&response);
    assert!(
        text.starts_with("Error: [ToolFailed] tool-not-in-profile:"),
        "{text}"
    );
}

/// A minimal JSON value of the structured type a schema property admits.
fn structured_sample(property: &Value) -> Option<Value> {
    match property.get("type").and_then(Value::as_str) {
        Some("array") => return Some(json!([])),
        Some("object") => return Some(json!({})),
        _ => {}
    }
    ["oneOf", "anyOf"]
        .iter()
        .filter_map(|key| property.get(key).and_then(Value::as_array))
        .flatten()
        .find_map(structured_sample)
}

/// Invariant: a call that follows a tool's own schema parses. Every property
/// the catalog declares as array / object (or a `oneOf` with one) is accepted
/// in that structured form; a schema that advertises it while the wire parser
/// rejects it (`spawn_agents.config`, `create_generator.params`, ...) fails
/// here, not in an agent's session.
#[test]
fn every_structured_argument_a_schema_advertises_parses() {
    let mut checked = 0;
    for schema in TOOL_SCHEMAS {
        let tool: Value = serde_json::from_str(schema).expect("schema json");
        let name = tool["name"].as_str().expect("name");
        let Some(properties) = tool["inputSchema"]["properties"].as_object() else {
            continue;
        };
        for (key, property) in properties {
            let Some(sample) = structured_sample(property) else {
                continue;
            };
            let line = call_line(name, json!({ key: sample }));
            let call = op_mcp::parse_tool_call(&line)
                .unwrap_or_else(|| panic!("{name}.{key} is advertised as structured but rejected"));
            assert!(call.arguments.contains_key(key), "{name}.{key}");
            checked += 1;
        }
    }
    assert!(
        checked >= 30,
        "the walk found only {checked} structured keys"
    );
}

/// Invariant: a schema-conformant `spawn_agents` call reaches the tool and
/// yields its sub-task specs (it was rejected at the parse before).
#[test]
fn a_schema_conformant_spawn_agents_call_parses_into_specs() {
    let line = call_line(
        "spawn_agents",
        json!({"config":[{"prompt":"Build the hero","containerNodes":["n1"],
                          "styleguideName":"minimal","guidelineNames":["landing-page"]}]}),
    );
    let call = op_mcp::parse_tool_call(&line).expect("parses");
    let specs = op_mcp::spawn_agents_tool::parse_spawn_config(&call.arguments).expect("specs");
    assert_eq!(specs.len(), 1);
}

/// Invariant: a schema-conformant generator call parses and the tool reads
/// the structured `params` / `frame` it was given.
#[test]
fn a_schema_conformant_create_generator_call_parses_with_params_and_frame() {
    let line = call_line(
        "create_generator",
        json!({"program":"return [];",
               "params":[{"name":"count","kind":"number","default":3}],
               "frame":{"width":300,"height":200}}),
    );
    let call = op_mcp::parse_tool_call(&line).expect("parses");
    assert!(call.arguments["params"].starts_with('['));
    assert!(call.arguments["frame"].starts_with('{'));
    let update = call_line(
        "update_generator_params",
        json!({"node_id":"g","params":{"count":4}}),
    );
    let call = op_mcp::parse_tool_call(&update).expect("parses");
    assert!(call.arguments["params"].starts_with('{'));
}
