//! Anthropic prompt caching for the request bodies this crate builds.
//!
//! Anthropic caches the request PREFIX in the order tools -> system ->
//! messages. A `cache_control: {"type":"ephemeral"}` marker on a block caches
//! everything up to and including that block (default life 5 minutes,
//! refreshed free on every hit; at most 4 markers per request; a prompt under
//! the model's minimum, 1,024 - 4,096 tokens, is simply not cached). The
//! lookback from a marker is 20 blocks, so a growing conversation keeps its
//! marker on the LAST block of the last message.
//!
//! [`apply_anthropic_cache_control`] places three markers: the last tool (the
//! whole tool list), the system prompt, and the last message's last content
//! block. It is applied to the per-request body only, never to the stored
//! conversation, so markers never pile up across turns.
//!
//! Only first-party Anthropic gets it ([`anthropic_cache_enabled_for_url`]):
//! a proxy or a compatible third-party endpoint may reject the extra field,
//! and the OpenAI-compatible wire never carries it.

use serde_json::{json, Value};

/// The only host that receives `cache_control`.
const ANTHROPIC_API_HOST: &str = "api.anthropic.com";

/// True when `url`'s host is exactly `api.anthropic.com` (parsed, not a
/// substring match: `api.anthropic.com.evil.test`, userinfo tricks and paths
/// naming the host do not count). A custom base URL, a proxy or a compatible
/// third-party endpoint stays untouched.
pub fn anthropic_cache_enabled_for_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .filter(|parsed| parsed.scheme() == "https")
        .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == ANTHROPIC_API_HOST)
}

fn marker() -> Value {
    json!({ "type": "ephemeral" })
}

/// Mark the cache breakpoints on an Anthropic `/v1/messages` body when
/// `enabled`: `system` (a string becomes a one-block array), the last tool,
/// and the last content block of the last message (a string content becomes a
/// one-block array). Original text is preserved byte for byte. Idempotent: a
/// second call re-sets the same markers. Does nothing when `enabled` is false.
pub fn apply_anthropic_cache_control(body: &mut Value, enabled: bool) {
    if !enabled {
        return;
    }
    let Some(object) = body.as_object_mut() else {
        return;
    };

    if let Some(last_tool) = object
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.last_mut())
        .and_then(Value::as_object_mut)
    {
        last_tool.insert("cache_control".into(), marker());
    }

    if let Some(system) = object.get_mut("system") {
        if let Some(text) = system.as_str() {
            // Cache control needs block form; an empty text block is refused.
            if !text.is_empty() {
                *system = json!([{ "type": "text", "text": text, "cache_control": marker() }]);
            }
        } else if let Some(last_block) = system
            .as_array_mut()
            .and_then(|blocks| blocks.last_mut())
            .and_then(Value::as_object_mut)
        {
            last_block.insert("cache_control".into(), marker());
        }
    }

    if let Some(message) = object
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|messages| messages.last_mut())
        .and_then(Value::as_object_mut)
    {
        if let Some(content) = message.get_mut("content") {
            if let Some(text) = content.as_str() {
                if !text.is_empty() {
                    *content = json!([{ "type": "text", "text": text, "cache_control": marker() }]);
                }
            } else if let Some(last_block) = content
                .as_array_mut()
                .and_then(|blocks| blocks.last_mut())
                .and_then(Value::as_object_mut)
            {
                let empty_text = last_block.get("type").and_then(Value::as_str) == Some("text")
                    && last_block.get("text").and_then(Value::as_str) == Some("");
                if !empty_text {
                    last_block.insert("cache_control".into(), marker());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "anthropic_cache_tests.rs"]
mod tests;
