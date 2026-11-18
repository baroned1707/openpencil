//! Anthropic prompt caching + usage accounting at the loop level.

use super::tests::{
    anthropic_tool_use_turn, serve_sse_script, update_node_tool_def, ScriptedExecutor,
};
use super::*;
use serde_json::Value;

fn config(url: &str) -> AgentLoopConfig {
    AgentLoopConfig {
        url: url.into(),
        api_key: "sk-test".into(),
        model: "claude-sonnet-4-5".into(),
        system_prompt: "You are a design agent.\nFollow the protocol.".into(),
        history: vec![(ChatHistoryRole::User, "earlier turn".into())],
        user_prompt: "make the title red".into(),
        attachments: Vec::new(),
        max_output_tokens: 512,
        tools: vec![update_node_tool_def(), update_node_tool_def()],
        executor: ScriptedExecutor::ok(r#"{"success":true,"data":{}}"#),
        max_turns: 5,
        finalize_on_exit: true,
        disable_thinking: false,
        dial_policy: crate::provider_dial::EndpointDialPolicy::Trusted,
    }
}

fn tools_json(cfg: &AgentLoopConfig) -> Vec<Value> {
    cfg.tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": serde_json::from_str::<Value>(&t.input_schema_json).unwrap(),
            })
        })
        .collect()
}

fn markers(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            usize::from(map.contains_key("cache_control"))
                + map.values().map(markers).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(markers).sum(),
        _ => 0,
    }
}

fn first_turn_messages() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": "earlier turn"}),
        json!({"role": "user", "content": "make the title red"}),
    ]
}

fn later_turn_messages() -> Vec<Value> {
    let mut messages = first_turn_messages();
    messages.push(json!({"role": "assistant", "content": [
        {"type": "tool_use", "id": "toolu_1", "name": "update_node", "input": {"nodeId": "n1"}}
    ]}));
    messages.push(json!({"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "toolu_1", "content": "{\"success\":true}", "is_error": false}
    ]}));
    messages
}

#[test]
fn the_loop_body_is_cached_only_for_api_anthropic_com() {
    let first_party = config("https://api.anthropic.com/v1/messages");
    let messages = later_turn_messages();
    let body = anthropic_turn_body(&first_party, &tools_json(&first_party), &messages);
    assert_eq!(markers(&body), 3);
    assert!(body["system"].is_array());
    assert_eq!(body["system"][0]["text"], first_party.system_prompt);
    // The persistent conversation is not touched: markers never accumulate.
    assert_eq!(markers(&Value::Array(messages)), 0);

    for url in [
        "http://127.0.0.1:9999/v1/messages",
        "https://api.minimax.io/anthropic/v1/messages",
        "https://my-proxy.example.com/v1/messages",
    ] {
        let other = config(url);
        let body = anthropic_turn_body(&other, &tools_json(&other), &first_turn_messages());
        assert_eq!(markers(&body), 0, "{url}");
        assert_eq!(body["system"], json!(other.system_prompt), "{url}");
    }
}

#[test]
fn loop_prefix_is_byte_stable_between_turns() {
    // Caching only pays if turn N+1 sends exactly the tools + system bytes of
    // turn N; only the messages grow.
    let cfg = config("https://api.anthropic.com/v1/messages");
    let tools = tools_json(&cfg);
    let first = anthropic_turn_body(&cfg, &tools, &first_turn_messages());
    let later = anthropic_turn_body(&cfg, &tools, &later_turn_messages());
    let prefix = |body: &Value| {
        format!(
            "{}|{}",
            serde_json::to_string(&body["tools"]).unwrap(),
            serde_json::to_string(&body["system"]).unwrap()
        )
    };
    assert_eq!(prefix(&first), prefix(&later));

    // And on the wire: two real requests of one run carry the same prefix.
    let (base, requests) = serve_sse_script(vec![anthropic_tool_use_turn(), text_turn()]);
    let wire = config(&format!("{base}/v1/messages"));
    let (outcome, _) = super::tests::run_loop_collect(wire, true);
    assert_eq!(outcome, Ok(true));
    let bodies: Vec<Value> = (0..2)
        .map(|_| {
            let request = requests
                .recv_timeout(std::time::Duration::from_secs(2))
                .expect("request");
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
        })
        .collect();
    assert_eq!(prefix(&bodies[0]), prefix(&bodies[1]));
    assert!(
        bodies[1]["messages"].as_array().unwrap().len()
            > bodies[0]["messages"].as_array().unwrap().len()
    );
    // A loopback endpoint is not first-party Anthropic: no markers on the wire.
    assert_eq!(markers(&bodies[0]), 0);
    assert_eq!(markers(&bodies[1]), 0);
}

fn text_turn() -> String {
    turn_with_usage("end_turn", None)
}

/// A text-only response; `usage` = (input, cache_write, cache_read, output).
fn turn_with_usage(stop: &str, usage: Option<(u64, u64, u64, u64)>) -> String {
    let mut lines = Vec::new();
    if let Some((input, write, read, _)) = usage {
        lines.push(format!(
            r#"data: {{"type":"message_start","message":{{"usage":{{"input_tokens":{input},"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":1}}}}}}"#
        ));
        lines.push(String::new());
    }
    lines.push(r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.into());
    lines.push(String::new());
    lines.push(r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#.into());
    lines.push(String::new());
    match usage {
        Some((_, _, _, output)) => lines.push(format!(
            r#"data: {{"type":"message_delta","delta":{{"stop_reason":"{stop}"}},"usage":{{"output_tokens":{output}}}}}"#
        )),
        None => lines.push(format!(
            r#"data: {{"type":"message_delta","delta":{{"stop_reason":"{stop}"}}}}"#
        )),
    }
    lines.push(String::new());
    lines.push(r#"data: {"type":"message_stop"}"#.into());
    lines.push(String::new());
    lines.push(String::new());
    lines.join("\n")
}

fn run_with_usage(
    turns: Vec<String>,
) -> (Result<bool, BuiltinHttpError>, op_ai::chat_usage::RunUsage) {
    let (base, _requests) = serve_sse_script(turns);
    let mut cfg = config(&format!("{base}/v1/messages"));
    // A text-only reply would otherwise owe a zero-write corrective round.
    cfg.finalize_on_exit = false;
    let (tx, mut rx) = mpsc::channel::<ChatDelta>(64);
    let mut usage = op_ai::chat_usage::RunUsage::default();
    let outcome = crate::chat_runtime::shared_runtime()
        .block_on(async { run_anthropic_agent_loop_inner(cfg, &tx, &mut usage).await });
    drop(tx);
    while rx.try_recv().is_ok() {}
    (outcome, usage)
}

#[test]
fn run_usage_adds_up_over_the_turns_of_a_loop() {
    // A tool turn, then a closing turn; the first reports usage, the second
    // reports cache reads.
    let tool_turn = anthropic_tool_use_turn().replacen(
        r#"data: {"type":"message_start"}"#,
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":100,"cache_creation_input_tokens":4000,"cache_read_input_tokens":0,"output_tokens":1}}}"#,
        1,
    ).replacen(
        r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":55}}"#,
        1,
    );
    let (outcome, usage) = run_with_usage(vec![
        tool_turn,
        turn_with_usage("end_turn", Some((20, 0, 4000, 30))),
    ]);
    assert_eq!(outcome, Ok(true));
    assert_eq!(usage.turns, 2);
    assert_eq!(usage.input_tokens, 120);
    assert_eq!(usage.cache_creation_input_tokens, 4000);
    assert_eq!(usage.cache_read_input_tokens, 4000);
    assert_eq!(usage.output_tokens, 85);
}

#[test]
fn a_run_whose_endpoint_reports_no_usage_counts_turns_and_zero_tokens() {
    let (outcome, usage) = run_with_usage(vec![text_turn()]);
    assert_eq!(outcome, Ok(true));
    assert_eq!(usage.turns, 1);
    assert_eq!(
        (
            usage.input_tokens,
            usage.cache_creation_input_tokens,
            usage.cache_read_input_tokens,
            usage.output_tokens
        ),
        (0, 0, 0, 0)
    );
}
