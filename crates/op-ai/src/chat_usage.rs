//! Token usage of an Anthropic streaming run.
//!
//! The Messages API reports usage twice per response: `message_start` carries
//! the input side (`input_tokens`, `cache_creation_input_tokens`,
//! `cache_read_input_tokens`) and an initial `output_tokens`; `message_delta`
//! carries the CUMULATIVE `output_tokens` (and may repeat the input fields).
//! Within one response every field is therefore taken as a running maximum, not
//! a sum; across the responses of a run ([`RunUsage::add_turn`]) fields add up.
//!
//! Nothing here touches the transport: callers feed raw SSE `data:` payloads.
//! A missing `usage` (a compatible endpoint that omits it, a truncated stream)
//! leaves every field at zero.

use serde_json::Value;

/// Token usage of one response, or of a whole run once turns are added.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunUsage {
    /// Fresh input tokens (not read from or written to the cache).
    pub input_tokens: u64,
    /// Input tokens written to the prompt cache.
    pub cache_creation_input_tokens: u64,
    /// Input tokens served from the prompt cache.
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
    /// Responses folded into this value: 0 or 1 for one response, N for a run.
    pub turns: u32,
}

impl RunUsage {
    /// Fold one SSE `data:` payload of a SINGLE response into this value.
    /// Payloads that are not `message_start` / `message_delta`, or carry no
    /// `usage`, change nothing.
    pub fn observe_anthropic_event(&mut self, data: &str) {
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let usage = match value.get("type").and_then(Value::as_str) {
            Some("message_start") => value.pointer("/message/usage"),
            Some("message_delta") => value.get("usage"),
            _ => None,
        };
        let Some(usage) = usage else {
            return;
        };
        let read = |key: &str| usage.get(key).and_then(Value::as_u64);
        let fields = [
            read("input_tokens"),
            read("cache_creation_input_tokens"),
            read("cache_read_input_tokens"),
            read("output_tokens"),
        ];
        // `usage: null`, or fields that are not numbers, report nothing.
        if fields.iter().all(Option::is_none) {
            return;
        }
        let slots = [
            &mut self.input_tokens,
            &mut self.cache_creation_input_tokens,
            &mut self.cache_read_input_tokens,
            &mut self.output_tokens,
        ];
        for (slot, seen) in slots.into_iter().zip(fields) {
            if let Some(seen) = seen {
                *slot = (*slot).max(seen);
            }
        }
        self.turns = 1;
    }

    /// Add a finished response's usage to this run total.
    pub fn add_turn(&mut self, turn: &RunUsage) {
        self.input_tokens += turn.input_tokens;
        self.cache_creation_input_tokens += turn.cache_creation_input_tokens;
        self.cache_read_input_tokens += turn.cache_read_input_tokens;
        self.output_tokens += turn.output_tokens;
        self.turns += turn.turns;
    }

    /// The one log line a run prints when it ends.
    pub fn log_line(&self) -> String {
        format!(
            "[CHAT] usage: input={} cache_write={} cache_read={} output={} turns={}",
            self.input_tokens,
            self.cache_creation_input_tokens,
            self.cache_read_input_tokens,
            self.output_tokens,
            self.turns
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":12,"cache_creation_input_tokens":3000,"cache_read_input_tokens":45000,"output_tokens":1}}}"#;
    const DELTA: &str = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":210}}"#;

    #[test]
    fn usage_is_parsed_from_streaming_events() {
        let mut turn = RunUsage::default();
        turn.observe_anthropic_event(START);
        turn.observe_anthropic_event(r#"{"type":"content_block_delta","index":0}"#);
        turn.observe_anthropic_event(DELTA);
        assert_eq!(
            turn,
            RunUsage {
                input_tokens: 12,
                cache_creation_input_tokens: 3000,
                cache_read_input_tokens: 45000,
                // The delta's cumulative count replaces the start's 1; it is not added.
                output_tokens: 210,
                turns: 1,
            }
        );
        // A newer API repeating the input fields in message_delta changes nothing.
        turn.observe_anthropic_event(
            r#"{"type":"message_delta","usage":{"input_tokens":12,"cache_read_input_tokens":45000,"output_tokens":210}}"#,
        );
        assert_eq!(turn.input_tokens, 12);
        assert_eq!(turn.cache_read_input_tokens, 45000);

        let mut run = RunUsage::default();
        run.add_turn(&turn);
        run.add_turn(&turn);
        assert_eq!(run.input_tokens, 24);
        assert_eq!(run.output_tokens, 420);
        assert_eq!(run.turns, 2);
        assert_eq!(
            run.log_line(),
            "[CHAT] usage: input=24 cache_write=6000 cache_read=90000 output=420 turns=2"
        );
    }

    #[test]
    fn missing_usage_is_zero_not_a_panic() {
        let mut turn = RunUsage::default();
        for data in [
            r#"{"type":"message_start","message":{"id":"msg_1"}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
            r#"{"type":"message_delta","usage":null}"#,
            r#"{"type":"message_delta","usage":{"output_tokens":"many"}}"#,
            "not json at all",
            "",
            "[DONE]",
        ] {
            turn.observe_anthropic_event(data);
        }
        assert_eq!(turn, RunUsage::default());
        assert_eq!(
            turn.log_line(),
            "[CHAT] usage: input=0 cache_write=0 cache_read=0 output=0 turns=0"
        );
        // Cache fields absent: only the fields present are read.
        turn.observe_anthropic_event(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":7,"output_tokens":2}}}"#,
        );
        assert_eq!(turn.input_tokens, 7);
        assert_eq!(turn.cache_creation_input_tokens, 0);
        assert_eq!(turn.cache_read_input_tokens, 0);
    }
}
