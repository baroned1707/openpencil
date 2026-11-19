//! Per-run shaping of what the design loop's model sees from its tools.
//!
//! [`DesignRunExecutor`] wraps the host's executor for ONE loop run and
//! changes two things on the way back to the model, leaving the host (and the
//! chat transcript card it attaches) untouched:
//!
//! - `get_guidelines` for a topic whose text is already in the run's system
//!   prompt (the prompt-matched domain skill) answers with a one-line
//!   reference instead of re-sending 7-18 KB the model already holds.
//! - A `batch_design` result's issue lists are shaped by the run's
//!   [`FeedbackLedger`]: a line the model was already shown and that is still
//!   present is replaced by an `(N unchanged ...)` count, because the earlier
//!   result, which carries the full line, is still in the conversation and
//!   re-sending a defect the model has not fixed yet only inflates every later
//!   turn. The count rides in `<key>Unchanged` (`layoutIssuesUnchanged: 2`).
//!   An issue that survives a third write in a row also adds one `replanHint`.
//!   Both rules, and how a fixed issue is allowed to come back, are described
//!   in `design_run_feedback.rs`.
//!
//! The loop-end completion gates (`finalize`, `check_unfilled_screens`,
//! `check_blockers`) are forwarded unchanged: they recompute from the live
//! document, never from these shaped results.

use std::sync::{Arc, Mutex};

use op_ai::chat_provider::{
    BlockerReport, ChatToolExecutor, ChatToolResult, FinalizeReport, UnfilledScreensReport,
};
use serde_json::{json, Value};

use crate::design_run_feedback::{page_key, FeedbackLedger};

pub struct DesignRunExecutor {
    inner: Arc<dyn ChatToolExecutor>,
    system_prompt: String,
    /// What the model has been shown of the write feedback so far this run.
    ledger: Mutex<FeedbackLedger>,
}

impl DesignRunExecutor {
    pub fn wrap(
        inner: Arc<dyn ChatToolExecutor>,
        system_prompt: &str,
    ) -> Arc<dyn ChatToolExecutor> {
        Arc::new(Self {
            inner,
            system_prompt: system_prompt.to_string(),
            ledger: Mutex::new(FeedbackLedger::default()),
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

    /// Shape a `batch_design` result through the run's [`FeedbackLedger`]:
    /// lines the model has already seen become a `<key>Unchanged` count, and
    /// an issue that survives its third write in a row adds one `replanHint`.
    /// `None` when the result is left as it came.
    fn shape_write_feedback(&self, args_json: &str, content: &str) -> Option<String> {
        let mut envelope: Value = serde_json::from_str(content).ok()?;
        let object = envelope.as_object_mut()?;
        let mut ledger = self.ledger.lock().ok()?;
        let changed = ledger.shape(&page_key(args_json), object);
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
                if let Some(shaped) = self.shape_write_feedback(args_json, &result.content) {
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

    fn usage_budget_exhausted(&self, run: &op_ai::chat_usage::RunUsage) -> bool {
        self.inner.usage_budget_exhausted(run)
    }
}

#[cfg(test)]
#[path = "design_run_executor_tests.rs"]
mod tests;
