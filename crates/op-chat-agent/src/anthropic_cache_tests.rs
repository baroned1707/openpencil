use super::*;

fn count_markers(value: &Value) -> usize {
    match value {
        Value::Object(map) => {
            usize::from(map.contains_key("cache_control"))
                + map.values().map(count_markers).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_markers).sum(),
        _ => 0,
    }
}

fn body() -> Value {
    json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 1024,
        "stream": true,
        "system": "SYSTEM TEXT \u{4e2d}\u{6587}\n  keep  spacing ",
        "tools": [
            { "name": "a", "description": "A", "input_schema": { "type": "object" } },
            { "name": "b", "description": "B", "input_schema": { "type": "object" } },
        ],
        "messages": [
            { "role": "user", "content": "first" },
            { "role": "assistant", "content": [{ "type": "text", "text": "reply" }] },
            { "role": "user", "content": [
                { "type": "tool_result", "tool_use_id": "t1", "content": "r1" },
                { "type": "tool_result", "tool_use_id": "t2", "content": "r2" },
            ] },
        ],
    })
}

#[test]
fn cache_control_has_three_breakpoints_in_the_right_places() {
    let original = body();
    let mut cached = original.clone();
    apply_anthropic_cache_control(&mut cached, true);

    assert_eq!(count_markers(&cached), 3);
    // system: array of one block, text preserved byte for byte.
    let system = cached["system"].as_array().expect("system is an array");
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["type"], "text");
    assert_eq!(system[0]["text"], original["system"]);
    assert_eq!(system[0]["cache_control"], json!({"type": "ephemeral"}));
    // tools: only the LAST tool.
    assert!(cached["tools"][0].get("cache_control").is_none());
    assert_eq!(
        cached["tools"][1]["cache_control"],
        json!({"type": "ephemeral"})
    );
    // messages: only the last block of the last message; earlier ones untouched.
    assert_eq!(cached["messages"][0], original["messages"][0]);
    assert_eq!(cached["messages"][1], original["messages"][1]);
    let last = cached["messages"][2]["content"].as_array().unwrap();
    assert!(last[0].get("cache_control").is_none());
    assert_eq!(last[1]["cache_control"], json!({"type": "ephemeral"}));
    // Everything else is unchanged.
    for key in ["model", "max_tokens", "stream"] {
        assert_eq!(cached[key], original[key]);
    }

    // Idempotent: a second pass neither adds markers nor changes the body.
    let once = cached.clone();
    apply_anthropic_cache_control(&mut cached, true);
    assert_eq!(cached, once);
    assert_eq!(count_markers(&cached), 3);
}

#[test]
fn a_string_last_message_becomes_one_marked_block_and_no_tools_gives_two() {
    let mut no_tools = json!({
        "model": "m",
        "system": "sys",
        "messages": [{ "role": "user", "content": "do the thing" }],
    });
    apply_anthropic_cache_control(&mut no_tools, true);
    assert_eq!(count_markers(&no_tools), 2);
    assert!(no_tools.get("tools").is_none());
    assert_eq!(
        no_tools["messages"][0]["content"],
        json!([{ "type": "text", "text": "do the thing", "cache_control": {"type": "ephemeral"} }])
    );

    // No system prompt: no empty block is invented for it.
    let mut no_system = json!({ "messages": [{ "role": "user", "content": "hi" }] });
    apply_anthropic_cache_control(&mut no_system, true);
    assert!(no_system.get("system").is_none());
    assert_eq!(count_markers(&no_system), 1);
}

#[test]
fn disabled_leaves_the_body_untouched() {
    let original = body();
    let mut untouched = original.clone();
    apply_anthropic_cache_control(&mut untouched, false);
    assert_eq!(untouched, original);
    assert!(untouched["system"].is_string());
}

#[test]
fn cache_control_only_for_api_anthropic_com() {
    for yes in [
        "https://api.anthropic.com/v1/messages",
        "https://API.ANTHROPIC.COM/v1/messages",
        "https://api.anthropic.com:443/v1/messages",
    ] {
        assert!(anthropic_cache_enabled_for_url(yes), "{yes}");
    }
    for no in [
        "http://127.0.0.1:8080/v1/messages",
        "https://api.anthropic.com.evil.test/v1/messages",
        "https://evil.test/api.anthropic.com/v1/messages",
        "https://api.anthropic.com@evil.test/v1/messages",
        "https://evil.test/?host=api.anthropic.com",
        "https://proxy.example.com/anthropic/v1/messages",
        "https://api.minimax.io/anthropic/v1/messages",
        "https://bedrock-runtime.us-east-1.amazonaws.com/v1/messages",
        "http://api.anthropic.com/v1/messages",
        "not a url",
        "",
    ] {
        assert!(!anthropic_cache_enabled_for_url(no), "{no}");
    }
}
