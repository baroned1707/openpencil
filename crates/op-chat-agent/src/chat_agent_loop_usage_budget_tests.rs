//! `ChatToolExecutor::usage_budget_exhausted`: a harness that bounds its
//! spend ends the run between requests, and the run still finalizes and
//! reports its usage before `Done`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use super::cache_tests::{config, turn_with_usage};
use super::tests::{run_loop_collect, serve_sse_script};
use super::*;

/// Runs every tool call successfully and says "budget used" once the run
/// has paid for `stop_after_turns` responses.
struct BudgetExecutor {
    stop_after_turns: u32,
    finalize_calls: AtomicUsize,
    asked_with_turns: Mutex<Vec<u32>>,
}

impl BudgetExecutor {
    fn new(stop_after_turns: u32) -> Arc<Self> {
        Arc::new(Self {
            stop_after_turns,
            finalize_calls: AtomicUsize::new(0),
            asked_with_turns: Mutex::new(Vec::new()),
        })
    }
}

impl ChatToolExecutor for BudgetExecutor {
    fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
        ChatToolResult {
            content: r#"{"success":true,"data":{}}"#.into(),
            is_error: false,
        }
    }

    fn finalize(&self) -> FinalizeReport {
        self.finalize_calls.fetch_add(1, Ordering::SeqCst);
        FinalizeReport::default()
    }

    fn usage_budget_exhausted(&self, run: &op_ai::chat_usage::RunUsage) -> bool {
        self.asked_with_turns.lock().unwrap().push(run.turns);
        run.turns >= self.stop_after_turns
    }
}

/// A tool-calling response that reports usage.
fn tool_turn_with_usage() -> String {
    turn_with_usage("tool_use", Some((100, 0, 900, 50))).replacen(
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"update_node"}}"#,
        1,
    )
    .replacen(
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"nodeId\":\"n1\"}"}}"#,
        1,
    )
}

#[test]
fn an_exhausted_budget_ends_the_run_before_the_next_request() {
    // Three responses are scripted; the budget allows only two.
    let (base, requests) = serve_sse_script(vec![
        tool_turn_with_usage(),
        tool_turn_with_usage(),
        tool_turn_with_usage(),
    ]);
    let executor = BudgetExecutor::new(2);
    let mut cfg = config(&format!("{base}/v1/messages"));
    cfg.executor = executor.clone();
    cfg.max_turns = 10;
    let (outcome, deltas) = run_loop_collect(cfg, true);
    assert_eq!(outcome, Ok(true));

    assert_eq!(
        requests.try_iter().count(),
        2,
        "no third request is paid for"
    );
    assert_eq!(
        executor.finalize_calls.load(Ordering::SeqCst),
        1,
        "the document is still finalized"
    );
    // The check runs before each request: with 0, 1 and then 2 finished turns.
    assert_eq!(*executor.asked_with_turns.lock().unwrap(), vec![0, 1, 2]);

    let usage: Vec<_> = deltas
        .iter()
        .enumerate()
        .filter_map(|(index, delta)| match delta {
            ChatDelta::Usage(run) => Some((index, *run)),
            _ => None,
        })
        .collect();
    assert_eq!(usage.len(), 1, "{deltas:?}");
    assert_eq!(usage[0].1.turns, 2);
    assert_eq!(usage[0].1.output_tokens, 100);
    let done = deltas.len() - 1;
    assert!(usage[0].0 < done, "usage precedes Done: {deltas:?}");
    assert!(matches!(
        deltas[done],
        ChatDelta::Done {
            stop_reason: StopReason::MaxTokens
        }
    ));
}

#[test]
fn the_default_executor_never_ends_a_run_on_usage() {
    struct Plain;
    impl ChatToolExecutor for Plain {
        fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
            ChatToolResult {
                content: String::new(),
                is_error: false,
            }
        }
    }
    let huge = op_ai::chat_usage::RunUsage {
        input_tokens: u64::MAX / 4,
        output_tokens: u64::MAX / 4,
        turns: 99,
        ..Default::default()
    };
    assert!(!Plain.usage_budget_exhausted(&huge));
}
