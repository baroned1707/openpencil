//! What happens between the model and the document during one recorded run:
//! the key scrubber, the token budget, and the executor that records every
//! tool call while the production [`HeadlessExecutor`] applies it.
//!
//! Budget unit: every token the model processed, i.e. `input_tokens +
//! cache_creation_input_tokens + cache_read_input_tokens + output_tokens` of
//! `RunUsage`. Cache reads are counted although they are cheaper than fresh
//! input: a long run re-reads its growing prompt every turn, so they are the
//! largest part of a run and the honest measure of how much a run did.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use op_ai::chat_provider::{
    BlockerReport, ChatDelta, ChatRequest, ChatToolExecutor, ChatToolResult, FinalizeReport,
    UnfilledScreensReport,
};
use op_ai::chat_usage::RunUsage;
use op_editor_core::EditorState;
use serde_json::{json, Value};

use crate::loop_mode::HeadlessExecutor;

/// A tool call's arguments are kept up to this many bytes in the transcript.
pub(crate) const TRANSCRIPT_ARGS_BYTES: usize = 4 * 1024;
/// A tool call's result is kept up to this many bytes in the transcript.
pub(crate) const TRANSCRIPT_RESULT_BYTES: usize = 2 * 1024;

/// The diagnostic keys a write result may carry (the same list the in-app
/// feedback uses); the timeline counts the entries of each.
const DIAGNOSTIC_KEYS: [&str; 10] = [
    "layoutIssues",
    "iconIssues",
    "fontIssues",
    "structureIssues",
    "intentQuestions",
    "variableIssues",
    "imageSlots",
    "contrastIssues",
    "contrastUncertain",
    "navIssues",
];

/// Removes the API key from every text the recorder writes. The key is held
/// only here, is never printed (`Debug` is redacted) and never leaves the
/// process except in the model request itself.
#[derive(Clone)]
pub(crate) struct Scrubber {
    secret: String,
}

impl std::fmt::Debug for Scrubber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Scrubber(<redacted>)")
    }
}

impl Scrubber {
    pub(crate) fn new(secret: &str) -> Self {
        Self {
            secret: secret.trim().to_string(),
        }
    }

    /// `text` with the key, and anything shaped like an Anthropic key,
    /// replaced by `[redacted]`.
    pub(crate) fn clean(&self, text: &str) -> String {
        let mut out = if self.secret.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.secret, "[redacted]")
        };
        while let Some(start) = out.find("sk-ant-") {
            let end = out[start..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
                .map_or(out.len(), |offset| start + offset);
            out.replace_range(start..end, "[redacted]");
        }
        out
    }
}

/// Tokens a run has used, in the budget's unit.
pub(crate) fn billable_tokens(usage: &RunUsage) -> u64 {
    usage.input_tokens
        + usage.cache_creation_input_tokens
        + usage.cache_read_input_tokens
        + usage.output_tokens
}

/// The caps one run is held to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunBudget {
    /// Tokens one run may use.
    pub max_run_tokens: u64,
    /// Tokens left for the whole invocation when this run starts.
    pub total_remaining: u64,
}

impl RunBudget {
    /// The first cap `usage` has reached, as a sentence, or `None`.
    pub(crate) fn reached(&self, usage: &RunUsage) -> Option<String> {
        let used = billable_tokens(usage);
        if used >= self.max_run_tokens {
            return Some(format!(
                "stopped at the per-run token cap ({used} of {})",
                self.max_run_tokens
            ));
        }
        if used >= self.total_remaining {
            return Some(format!(
                "stopped at the total token cap ({used} used by this run, {} were left)",
                self.total_remaining
            ));
        }
        None
    }
}

/// How the model is reached. The real one calls the Anthropic API; tests
/// script the calls. `max_turns` is the per-run turn cap.
pub(crate) trait RunTransport: Sync {
    fn run(
        &self,
        request: ChatRequest,
        executor: Arc<dyn ChatToolExecutor>,
        max_turns: usize,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send>;
}

/// Everything recorded about one run, read back after the loop ends.
#[derive(Default)]
struct Log {
    transcript: Vec<Value>,
    pending_text: String,
    tool_counts: BTreeMap<String, u32>,
    timeline: Vec<Value>,
    writes: u32,
    finalize: Option<FinalizeReport>,
}

pub(crate) struct RecordingExecutor {
    inner: HeadlessExecutor,
    state: Arc<Mutex<EditorState>>,
    budget: RunBudget,
    scrub: Scrubber,
    log: Mutex<Log>,
    stop_reason: Mutex<Option<String>>,
}

/// What a run left behind.
pub(crate) struct Recorded {
    pub transcript: Vec<Value>,
    pub tool_counts: BTreeMap<String, u32>,
    pub timeline: Vec<Value>,
    pub finalize: Option<FinalizeReport>,
    pub stop_reason: Option<String>,
}

impl RecordingExecutor {
    pub(crate) fn new(
        state: Arc<Mutex<EditorState>>,
        prompt: &str,
        budget: RunBudget,
        scrub: Scrubber,
    ) -> Self {
        Self {
            inner: HeadlessExecutor::with_prompt(state.clone(), false, prompt),
            state,
            budget,
            scrub,
            log: Mutex::new(Log::default()),
            stop_reason: Mutex::new(None),
        }
    }

    /// Assistant text, in the order it was streamed.
    pub(crate) fn note_text(&self, text: &str) {
        self.log.lock().expect("log").pending_text.push_str(text);
    }

    pub(crate) fn take_recorded(&self) -> Recorded {
        let mut log = self.log.lock().expect("log");
        flush_text(&mut log, &self.scrub);
        Recorded {
            transcript: std::mem::take(&mut log.transcript),
            tool_counts: std::mem::take(&mut log.tool_counts),
            timeline: std::mem::take(&mut log.timeline),
            finalize: log.finalize.take(),
            stop_reason: self.stop_reason.lock().expect("stop").take(),
        }
    }
}

fn flush_text(log: &mut Log, scrub: &Scrubber) {
    if log.pending_text.trim().is_empty() {
        log.pending_text.clear();
        return;
    }
    let text = scrub.clean(&std::mem::take(&mut log.pending_text));
    log.transcript
        .push(json!({ "type": "assistant_text", "text": text }));
}

/// `text` cut to at most `max` bytes on a character boundary, with a marker.
pub(crate) fn truncate_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated, {} bytes]", &text[..end], text.len())
}

/// Counts of each diagnostic list in a write result (only non-empty ones).
fn diagnostics_of(content: &str) -> Value {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return json!({});
    };
    let mut found = serde_json::Map::new();
    for scope in [Some(&value), value.get("data")].into_iter().flatten() {
        for key in DIAGNOSTIC_KEYS {
            let len = scope.get(key).and_then(Value::as_array).map_or(0, Vec::len);
            if len > 0 {
                found.insert(key.to_string(), json!(len));
            }
        }
    }
    Value::Object(found)
}

impl ChatToolExecutor for RecordingExecutor {
    fn execute(&self, name: &str, args_json: &str) -> ChatToolResult {
        let result = self.inner.execute(name, args_json);
        let shown_result = if name == "get_screenshot" {
            format!("<image, {} base64 chars>", result.content.len())
        } else {
            truncate_bytes(&result.content, TRANSCRIPT_RESULT_BYTES)
        };
        let mut log = self.log.lock().expect("log");
        flush_text(&mut log, &self.scrub);
        *log.tool_counts.entry(name.to_string()).or_insert(0) += 1;
        log.transcript.push(json!({
            "type": "tool_call",
            "tool": name,
            "args": self.scrub.clean(&truncate_bytes(args_json, TRANSCRIPT_ARGS_BYTES)),
            "result": self.scrub.clean(&shown_result),
            "isError": result.is_error,
        }));
        if name == "batch_design" {
            log.writes += 1;
            let mut entry = json!({
                "write": log.writes,
                "isError": result.is_error,
                "diagnostics": diagnostics_of(&result.content),
            });
            if !result.is_error {
                entry["nodeCount"] = json!(self.node_count());
            }
            log.timeline.push(entry);
        }
        result
    }

    fn finalize(&self) -> FinalizeReport {
        let report = self.inner.finalize();
        self.log.lock().expect("log").finalize = Some(report.clone());
        report
    }

    fn check_unfilled_screens(&self) -> UnfilledScreensReport {
        self.inner.check_unfilled_screens()
    }

    fn check_blockers(&self) -> BlockerReport {
        self.inner.check_blockers()
    }

    fn usage_budget_exhausted(&self, run: &RunUsage) -> bool {
        match self.budget.reached(run) {
            Some(reason) => {
                *self.stop_reason.lock().expect("stop") = Some(reason);
                true
            }
            None => false,
        }
    }
}

impl RecordingExecutor {
    fn node_count(&self) -> usize {
        self.state.lock().map_or(0, |state| {
            serde_json::to_value(&state.doc)
                .ok()
                .map_or(0, |doc| count_nodes(&doc))
        })
    }
}

fn count_nodes(value: &Value) -> usize {
    match value {
        Value::Object(fields) => {
            usize::from(fields.contains_key("id") && fields.contains_key("type"))
                + fields.values().map(count_nodes).sum::<usize>()
        }
        Value::Array(items) => items.iter().map(count_nodes).sum(),
        _ => 0,
    }
}
