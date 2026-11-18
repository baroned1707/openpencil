//! The non-loop Anthropic request body: prompt caching is first-party only.

use super::*;

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

fn body_for(url: &str, system_prompt: &str) -> Value {
    anthropic_chat_body(
        "claude-sonnet-4-5",
        1024,
        vec![json!({"role": "user", "content": "hello"})],
        system_prompt,
        url,
    )
}

#[test]
fn the_non_loop_body_is_cached_only_for_api_anthropic_com() {
    let cached = body_for("https://api.anthropic.com/v1/messages", "SYSTEM");
    // No tools on this path: system + the last message.
    assert_eq!(markers(&cached), 2);
    assert_eq!(cached["system"][0]["text"], "SYSTEM");
    assert_eq!(
        cached["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(cached["messages"][0]["content"][0]["text"], "hello");

    for url in [
        "http://127.0.0.1:8080/v1/messages",
        "https://proxy.example.com/v1/messages",
        "https://api.minimax.io/anthropic/v1/messages",
    ] {
        let plain = body_for(url, "SYSTEM");
        assert_eq!(markers(&plain), 0, "{url}");
        assert_eq!(plain["system"], "SYSTEM", "{url}");
        assert_eq!(plain["messages"][0]["content"], "hello", "{url}");
    }
    // No system prompt: the key is absent, as before.
    assert!(body_for("https://api.anthropic.com/v1/messages", "  ")
        .get("system")
        .is_none());
}
