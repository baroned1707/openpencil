//! Token usage reported by a turn lands on the chat tab the turn is bound to.

use super::*;
use op_ai::chat_provider::{ChatDelta, StopReason};
use op_ai::chat_usage::RunUsage;
use op_editor_core::{ChatMessage, UsageView};

fn run(input: u64, cache_read: u64, output: u64) -> RunUsage {
    RunUsage {
        input_tokens: input,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: cache_read,
        output_tokens: output,
        turns: 2,
    }
}

#[test]
fn usage_lands_on_the_bound_tab_and_agents_of_a_turn_add_up() {
    let mut host = WidgetHostNative::new();
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant_streaming());
    // The user opens a second tab while tab 0's turn is still running.
    host.editor_state_mut().chat.new_tab();
    assert_eq!(host.editor_state().chat.active_index(), 1);

    let (primary_tx, primary_rx) = std::sync::mpsc::channel();
    let mut primary = Some(ChatSession::from_channels(primary_rx, None));
    primary_tx
        .send(ChatDelta::Usage(run(100, 900, 40)))
        .expect("usage delta queued");
    primary_tx
        .send(ChatDelta::Done {
            stop_reason: StopReason::EndTurn,
        })
        .expect("done queued");
    assert!(pump(
        &mut host,
        &mut primary,
        Some(0),
        None,
        (1200.0, 800.0)
    ));
    assert!(primary.is_none(), "the turn finished");

    let chat = &host.editor_state().chat;
    assert_eq!(
        chat.tabs()[0].last_run_usage,
        Some(UsageView {
            input: 100,
            cache_read: 900,
            cache_write: 0,
            output: 40,
            turns: 2,
        })
    );
    assert_eq!(
        chat.tabs()[1].last_run_usage,
        None,
        "the active tab did not run"
    );

    // A second agent of the same turn adds to the sum.
    host.editor_state_mut().chat.tabs_mut()[0]
        .messages
        .push(ChatMessage::assistant_streaming());
    let (second_tx, second_rx) = std::sync::mpsc::channel();
    let mut second = Some(ChatSession::from_channels(second_rx, None));
    second_tx
        .send(ChatDelta::Usage(run(20, 0, 5)))
        .expect("usage delta queued");
    assert!(pump(
        &mut host,
        &mut second,
        Some(0),
        Some(("Agent B", "#ff0000")),
        (1200.0, 800.0)
    ));
    let total = host.editor_state().chat.tabs()[0]
        .last_run_usage
        .expect("summed usage");
    assert_eq!(
        (total.input, total.cache_read, total.output, total.turns),
        (120, 900, 45, 4)
    );
}
