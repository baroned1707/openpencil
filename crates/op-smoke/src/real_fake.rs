//! Test doubles for the recorder: scripted "models" that drive the real tool
//! executor without any network.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use op_ai::chat_provider::{ChatDelta, ChatRequest, ChatToolExecutor, StopReason};
use op_ai::chat_usage::RunUsage;

use super::real_executor::RunTransport;
use crate::eval::{eval_replay::step_args, Fixture};

/// What a scripted model does for one prompt.
pub(super) enum Script {
    /// Call each `(tool, args)` in turn; every call costs `per_call` tokens.
    Steps {
        calls: Vec<(String, String)>,
        per_call: (u64, u64),
        /// Text put into the assistant text and the first call's arguments,
        /// to prove the scrubber removes it.
        leak: Option<String>,
    },
    /// The provider reports this error.
    Fail(String),
}

#[derive(Default)]
pub(super) struct FakeTransport {
    scripts: HashMap<String, Script>,
    pub started: AtomicUsize,
}

impl FakeTransport {
    pub(super) fn with(mut self, prompt: &str, script: Script) -> Self {
        self.scripts.insert(prompt.to_string(), script);
        self
    }
}

/// The scripted calls of an eval fixture.
pub(super) fn calls_of(fixture: &Fixture) -> Vec<(String, String)> {
    fixture
        .file
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let args = step_args(fixture, index, step).expect("fixture step args");
            (step.tool.clone(), args)
        })
        .collect()
}

impl RunTransport for FakeTransport {
    fn run(
        &self,
        request: ChatRequest,
        executor: Arc<dyn ChatToolExecutor>,
        _max_turns: usize,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.started.fetch_add(1, Ordering::SeqCst);
        let mut deltas = Vec::new();
        match self.scripts.get(&request.user_message) {
            Some(Script::Steps {
                calls,
                per_call,
                leak,
            }) => {
                let mut usage = RunUsage::default();
                for (index, (tool, args)) in calls.iter().enumerate() {
                    if executor.usage_budget_exhausted(&usage) {
                        break;
                    }
                    let mut args = args.clone();
                    if let (0, Some(secret)) = (index, leak) {
                        deltas.push(ChatDelta::TextDelta(format!("my key is {secret}")));
                        args = format!(r#"{{"note":"{secret}"}}"#);
                    }
                    deltas.push(ChatDelta::ToolUse {
                        name: tool.clone(),
                        args: args.clone(),
                    });
                    let _ = executor.execute(tool, &args);
                    usage.add_turn(&RunUsage {
                        input_tokens: per_call.0,
                        output_tokens: per_call.1,
                        turns: 1,
                        ..RunUsage::default()
                    });
                }
                let _ = executor.finalize();
                deltas.push(ChatDelta::Usage(usage));
                deltas.push(ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                });
            }
            Some(Script::Fail(text)) => {
                deltas.push(ChatDelta::Error(text.clone()));
                deltas.push(ChatDelta::Done {
                    stop_reason: StopReason::Aborted,
                });
            }
            None => deltas.push(ChatDelta::Error("no script for this prompt".into())),
        }
        Box::new(deltas.into_iter())
    }
}

/// A transport that must never be used (proves `--plan` makes no call).
pub(super) struct PanicTransport;

impl RunTransport for PanicTransport {
    fn run(
        &self,
        _request: ChatRequest,
        _executor: Arc<dyn ChatToolExecutor>,
        _max_turns: usize,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        panic!("the transport was used");
    }
}
