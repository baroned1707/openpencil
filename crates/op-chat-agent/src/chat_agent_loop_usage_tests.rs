//! The usage delta the loop hands to the chat UI: once per run, before `Done`.

use super::cache_tests::{config, text_turn, turn_with_usage};
use super::tests::{anthropic_tool_use_turn, run_loop_collect, serve_sse_script};
use super::*;

fn usage_deltas(deltas: &[ChatDelta]) -> Vec<(usize, op_ai::chat_usage::RunUsage)> {
    deltas
        .iter()
        .enumerate()
        .filter_map(|(index, delta)| match delta {
            ChatDelta::Usage(run) => Some((index, *run)),
            _ => None,
        })
        .collect()
}

fn done_positions(deltas: &[ChatDelta]) -> Vec<usize> {
    deltas
        .iter()
        .enumerate()
        .filter(|(_, delta)| matches!(delta, ChatDelta::Done { .. }))
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn usage_delta_is_sent_once_per_run_and_before_done() {
    // A tool turn then a closing turn, both reporting usage.
    let tool_turn = anthropic_tool_use_turn()
        .replacen(
            r#"data: {"type":"message_start"}"#,
            r#"data: {"type":"message_start","message":{"usage":{"input_tokens":100,"cache_creation_input_tokens":4000,"cache_read_input_tokens":0,"output_tokens":1}}}"#,
            1,
        )
        .replacen(
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":55}}"#,
            1,
        );
    let (base, _requests) = serve_sse_script(vec![
        tool_turn,
        turn_with_usage("end_turn", Some((20, 0, 4000, 30))),
    ]);
    let mut cfg = config(&format!("{base}/v1/messages"));
    cfg.finalize_on_exit = false;
    let (outcome, deltas) = run_loop_collect(cfg, true);
    assert_eq!(outcome, Ok(true));

    let usage = usage_deltas(&deltas);
    assert_eq!(usage.len(), 1, "one usage delta per run: {deltas:?}");
    let (usage_at, run) = usage[0];
    assert_eq!(run.turns, 2);
    assert_eq!(run.input_tokens, 120);
    assert_eq!(run.cache_creation_input_tokens, 4000);
    assert_eq!(run.cache_read_input_tokens, 4000);
    assert_eq!(run.output_tokens, 85);

    let done = done_positions(&deltas);
    assert_eq!(done.len(), 1, "one terminal Done: {deltas:?}");
    assert!(usage_at < done[0], "usage must precede Done: {deltas:?}");
    assert_eq!(done[0], deltas.len() - 1, "Done is the last delta");
}

#[test]
fn run_without_usage_sends_no_delta() {
    // The endpoint reports no `usage`: turns are counted, tokens are zero, and
    // the UI must not be told to show a zero meter.
    let (base, _requests) = serve_sse_script(vec![text_turn()]);
    let mut cfg = config(&format!("{base}/v1/messages"));
    cfg.finalize_on_exit = false;
    let (outcome, deltas) = run_loop_collect(cfg, true);
    assert_eq!(outcome, Ok(true));
    assert!(
        usage_deltas(&deltas).is_empty(),
        "no token data, no delta: {deltas:?}"
    );
    assert_eq!(done_positions(&deltas).len(), 1);
}

#[test]
fn usage_is_sent_once_when_the_loop_fails_before_done() {
    // An in-stream error ends the loop with `Err`; the caller adds
    // `Error` + `Done` after the wrapper returns, so usage must already be out.
    let failing = [
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":7,"output_tokens":1}}}"#,
        "",
        r#"data: {"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#,
        "",
        "",
    ]
    .join("\n");
    let (base, _requests) = serve_sse_script(vec![failing]);
    let mut cfg = config(&format!("{base}/v1/messages"));
    cfg.finalize_on_exit = false;
    let (outcome, deltas) = run_loop_collect(cfg, true);
    assert!(outcome.is_err(), "the stream error fails the run");
    let usage = usage_deltas(&deltas);
    assert_eq!(usage.len(), 1, "{deltas:?}");
    assert_eq!(usage[0].1.input_tokens, 7);
    assert!(done_positions(&deltas).is_empty(), "the caller owns Done");
}
