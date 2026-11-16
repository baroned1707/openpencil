//! Idempotent retries for write calls.
//!
//! A client that loses a connection mid-call cannot tell whether the write
//! landed. It retries with the same `operationId`; the server then returns
//! the stored response instead of applying a second time. One log per
//! document, bounded in size and age, so a long-running agent cannot grow it
//! without limit and a stale id cannot replay an hours-old answer.
//!
//! Only two outcomes are remembered: a call that was applied (its response is
//! replayed) and a call whose outcome is in doubt (the UI thread started it
//! and never answered, so it may have landed; a retry is refused rather than
//! risk applying twice). A call that never ran (the UI thread was busy and the
//! request was cancelled) is NOT remembered: its id stays free for the retry.
//!
//! The log takes `now` as an argument instead of reading a clock, so the
//! expiry rules are tested without sleeping.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Most operations remembered per document.
pub(super) const OPERATION_LOG_CAPACITY: usize = 256;
/// How long an operation is remembered.
pub(super) const OPERATION_LOG_TTL: Duration = Duration::from_secs(10 * 60);

/// What one call asked for, reduced to a comparable number: the tool and its
/// arguments (without the addressing ones). Reusing an `operationId` with a
/// different fingerprint is a client bug, not a retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CallFingerprint(u64);

impl CallFingerprint {
    pub(super) fn of(tool: &str, arguments: &std::collections::BTreeMap<String, String>) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tool.hash(&mut hasher);
        arguments.hash(&mut hasher);
        Self(hasher.finish())
    }
}

/// The answer to "have we seen this operation?".
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Lookup {
    /// Never seen (or expired / evicted): run the call.
    Miss,
    /// Seen with the same call: return this stored response, do not apply.
    Replay(String),
    /// Seen with the same call, but whether it landed is unknown.
    InDoubt,
    /// Seen with a different call.
    Conflict,
}

enum Outcome {
    Applied(String),
    InDoubt,
}

struct Entry {
    fingerprint: CallFingerprint,
    outcome: Outcome,
    stored_at: Instant,
    last_used: u64,
}

#[derive(Default)]
pub(super) struct OperationLog {
    entries: HashMap<String, Entry>,
    /// Monotonic use counter: the entry with the smallest value is the least
    /// recently used.
    tick: u64,
}

impl OperationLog {
    pub(super) fn lookup(
        &mut self,
        operation_id: &str,
        fingerprint: CallFingerprint,
        now: Instant,
    ) -> Lookup {
        self.expire(now);
        self.tick += 1;
        let tick = self.tick;
        let Some(entry) = self.entries.get_mut(operation_id) else {
            return Lookup::Miss;
        };
        if entry.fingerprint != fingerprint {
            return Lookup::Conflict;
        }
        entry.last_used = tick;
        match &entry.outcome {
            Outcome::Applied(response) => Lookup::Replay(response.clone()),
            Outcome::InDoubt => Lookup::InDoubt,
        }
    }

    /// Remember a successful response, evicting the least recently used
    /// entry when the log is full.
    pub(super) fn record(
        &mut self,
        operation_id: &str,
        fingerprint: CallFingerprint,
        response: String,
        now: Instant,
    ) {
        self.store(operation_id, fingerprint, Outcome::Applied(response), now);
    }

    /// Remember that the call may or may not have landed.
    pub(super) fn record_in_doubt(
        &mut self,
        operation_id: &str,
        fingerprint: CallFingerprint,
        now: Instant,
    ) {
        self.store(operation_id, fingerprint, Outcome::InDoubt, now);
    }

    fn store(
        &mut self,
        operation_id: &str,
        fingerprint: CallFingerprint,
        outcome: Outcome,
        now: Instant,
    ) {
        self.expire(now);
        if !self.entries.contains_key(operation_id) && self.entries.len() >= OPERATION_LOG_CAPACITY
        {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.tick += 1;
        self.entries.insert(
            operation_id.to_string(),
            Entry {
                fingerprint,
                outcome,
                stored_at: now,
                last_used: self.tick,
            },
        );
    }

    fn expire(&mut self, now: Instant) {
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.stored_at) < OPERATION_LOG_TTL);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// The stored response for a retry: same answer, the retry's own JSON-RPC id,
/// and `result._meta.replayed = true` so the client can tell. `_meta` is the
/// MCP extension point; strict validators reject extra top-level result keys.
pub(super) fn replay_response(stored: &str, retry_id: &op_mcp::RequestId) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(stored) else {
        return stored.to_string();
    };
    value["id"] = match retry_id {
        op_mcp::RequestId::Num(number) => serde_json::Value::from(*number),
        op_mcp::RequestId::Str(text) => serde_json::Value::from(text.as_str()),
    };
    if let Some(result) = value.get_mut("result").and_then(|r| r.as_object_mut()) {
        let meta = result
            .entry("_meta")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert("replayed".to_string(), serde_json::Value::Bool(true));
        }
    }
    value.to_string()
}

#[cfg(test)]
#[path = "operation_log_tests.rs"]
mod tests;
