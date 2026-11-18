//! Per-run shaping of what the design loop's model sees from its tools.
//!
//! [`DesignRunExecutor`] wraps the host's executor for ONE loop run and
//! changes two things on the way back to the model, leaving the host (and the
//! chat transcript card it attaches) untouched:
//!
//! - `get_guidelines` for a topic whose text is already in the run's system
//!   prompt (the prompt-matched domain skill) answers with a one-line
//!   reference instead of re-sending 7-18 KB the model already holds.
//! - A `batch_design` result's issue lists drop lines that are byte-identical
//!   to the previous write's, replaced by an `(N unchanged ...)` count: the
//!   earlier result, which carries the full lines, is still in the
//!   conversation, and re-sending a defect the model has not fixed yet only
//!   inflates every later turn. The count rides in `<key>Unchanged`
//!   (`layoutIssuesUnchanged: 2`), the entries themselves are only in an
//!   earlier result.
//!
//! The loop-end completion gates (`finalize`, `check_unfilled_screens`,
//! `check_blockers`) are forwarded unchanged: they recompute from the live
//! document, never from these shaped results.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use op_ai::chat_provider::{
    BlockerReport, ChatToolExecutor, ChatToolResult, FinalizeReport, UnfilledScreensReport,
};
use serde_json::{json, Value};

use crate::design_agent_tools::LIST_KEYS;

pub struct DesignRunExecutor {
    inner: Arc<dyn ChatToolExecutor>,
    system_prompt: String,
    /// Issue entries of the previous `batch_design` result, per list key.
    previous_lines: Mutex<HashMap<String, Vec<Value>>>,
}

impl DesignRunExecutor {
    pub fn wrap(
        inner: Arc<dyn ChatToolExecutor>,
        system_prompt: &str,
    ) -> Arc<dyn ChatToolExecutor> {
        Arc::new(Self {
            inner,
            system_prompt: system_prompt.to_string(),
            previous_lines: Mutex::new(HashMap::new()),
        })
    }

    /// `Some(topic)` when `args_json` asks for a guide topic whose every
    /// component skill text is already inside the system prompt. A prompt that
    /// holds only part of the topic (budget-trimmed, or a different skill mix)
    /// does not count: the model then still gets the full text.
    fn covered_guideline_topic(&self, args_json: &str) -> Option<String> {
        let args: Value = serde_json::from_str(args_json).ok()?;
        let category = args
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("guide");
        if !category.trim().is_empty() && !category.trim().eq_ignore_ascii_case("guide") {
            return None;
        }
        let topic = args.get("topic")?.as_str()?.trim().to_string();
        let components = op_ai_skills::guideline_components(&topic);
        (!components.is_empty()
            && components
                .iter()
                .all(|component| self.system_prompt.contains(component.as_str())))
        .then_some(topic)
    }

    /// Drop issue entries identical to the previous write's. Entries are
    /// compared as JSON values, so the structured `contrastIssues` objects
    /// dedupe like the string lines. The dropped count rides in a sibling
    /// `<key>Unchanged` number; a list that empties out is removed.
    fn dedupe_write_feedback(&self, content: &str) -> Option<String> {
        let mut envelope: Value = serde_json::from_str(content).ok()?;
        let object = envelope.as_object_mut()?;
        let mut previous = self.previous_lines.lock().ok()?;
        let mut changed = false;
        let mut seen_now: HashMap<String, Vec<Value>> = HashMap::new();
        for key in LIST_KEYS {
            let Some(items) = object.get(key).and_then(Value::as_array).cloned() else {
                continue;
            };
            let before = previous.get(key);
            let (repeated, fresh): (Vec<&Value>, Vec<&Value>) = items
                .iter()
                .partition(|item| before.is_some_and(|prev| prev.contains(item)));
            if !repeated.is_empty() {
                let count = repeated.len();
                if fresh.is_empty() {
                    object.remove(key);
                } else {
                    object.insert(
                        key.to_string(),
                        Value::Array(fresh.into_iter().cloned().collect()),
                    );
                }
                object.insert(format!("{key}Unchanged"), json!(count));
                changed = true;
            }
            seen_now.insert(key.to_string(), items);
        }
        // A key absent from this write was fixed; if it returns it is new.
        *previous = seen_now;
        changed.then(|| envelope.to_string())
    }
}

impl ChatToolExecutor for DesignRunExecutor {
    fn execute(&self, name: &str, args_json: &str) -> ChatToolResult {
        let mut result = self.inner.execute(name, args_json);
        if result.is_error {
            return result;
        }
        match name {
            "get_guidelines" => {
                if let Some(topic) = self.covered_guideline_topic(args_json) {
                    if let Ok(mut envelope) = serde_json::from_str::<Value>(&result.content) {
                        if let Some(content) = envelope.pointer_mut("/data/content") {
                            *content = json!(format!(
                                "This guideline is already in your instructions: {topic}"
                            ));
                            result.content = envelope.to_string();
                        }
                    }
                }
            }
            "batch_design" => {
                if let Some(shaped) = self.dedupe_write_feedback(&result.content) {
                    result.content = shaped;
                }
            }
            _ => {}
        }
        result
    }

    fn finalize(&self) -> FinalizeReport {
        self.inner.finalize()
    }

    fn check_unfilled_screens(&self) -> UnfilledScreensReport {
        self.inner.check_unfilled_screens()
    }

    fn check_blockers(&self) -> BlockerReport {
        self.inner.check_blockers()
    }
}

#[cfg(test)]
#[path = "design_run_executor_tests.rs"]
mod tests;
