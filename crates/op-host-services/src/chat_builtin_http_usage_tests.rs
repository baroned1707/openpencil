//! The usage delta of the non-loop Anthropic chat path: once, before `Done`.

use super::tests::read_http_request;
use super::*;
use std::io::Write;
use std::net::TcpListener;
use std::time::Duration;

const MESSAGE_START: &str = r#"data: {"type":"message_start","message":{"usage":{"input_tokens":12,"cache_creation_input_tokens":3000,"cache_read_input_tokens":45000,"output_tokens":1}}}"#;
const TEXT: &str =
    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#;
const MESSAGE_DELTA: &str = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":210}}"#;
const MESSAGE_STOP: &str = r#"data: {"type":"message_stop"}"#;

/// Serve `events` (each followed by a blank line) to one request and collect
/// every delta the provider yields.
fn run_chat_against(events: &[&str]) -> Vec<ChatDelta> {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local SSE server");
    let addr = listener.local_addr().expect("local addr");
    let body: String = events.iter().map(|event| format!("{event}\n\n")).collect();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let _ = read_http_request(&mut stream);
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write SSE response");
    });
    let provider = ConfiguredBuiltinProvider::from_builtin_agent(&BuiltinAgentConfig {
        id: "builtin-1".into(),
        preset: op_editor_core::BuiltinAgentPresetKey::Custom,
        display_name: "Mock Anthropic".into(),
        kind: BuiltinAgentKind::Anthropic,
        api_key: "sk-test".into(),
        models: vec!["claude-test".into()],
        base_url: format!("http://{addr}"),
        enabled: true,
    })
    .expect("ready provider");
    let deltas: Vec<ChatDelta> = provider
        .send(ChatRequest {
            user_message: "hello".into(),
            max_output_tokens: 64,
            ..Default::default()
        })
        .collect();
    server.join().expect("server thread exits");
    deltas
}

fn usage_positions(deltas: &[ChatDelta]) -> Vec<usize> {
    deltas
        .iter()
        .enumerate()
        .filter(|(_, delta)| matches!(delta, ChatDelta::Usage(_)))
        .map(|(index, _)| index)
        .collect()
}

#[test]
fn chat_usage_delta_is_sent_once_and_before_done() {
    let deltas = run_chat_against(&[MESSAGE_START, TEXT, MESSAGE_DELTA, MESSAGE_STOP]);
    let usage = usage_positions(&deltas);
    assert_eq!(usage.len(), 1, "{deltas:?}");
    let Some(ChatDelta::Usage(run)) = deltas.get(usage[0]) else {
        unreachable!()
    };
    assert_eq!(run.input_tokens, 12);
    assert_eq!(run.cache_creation_input_tokens, 3000);
    assert_eq!(run.cache_read_input_tokens, 45000);
    assert_eq!(run.output_tokens, 210);
    assert!(
        matches!(deltas.last(), Some(ChatDelta::Done { .. })),
        "{deltas:?}"
    );
    assert_eq!(usage[0], deltas.len() - 2, "usage directly precedes Done");
}

#[test]
fn chat_run_without_usage_sends_no_delta() {
    let deltas = run_chat_against(&[
        TEXT,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
        MESSAGE_STOP,
    ]);
    assert!(usage_positions(&deltas).is_empty(), "{deltas:?}");
    assert!(matches!(deltas.last(), Some(ChatDelta::Done { .. })));
}

#[test]
fn chat_usage_precedes_a_stream_error_and_its_done() {
    let deltas = run_chat_against(&[
        MESSAGE_START,
        r#"data: {"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#,
    ]);
    let usage = usage_positions(&deltas);
    assert_eq!(usage.len(), 1, "{deltas:?}");
    let error = deltas
        .iter()
        .position(|delta| matches!(delta, ChatDelta::Error(_)))
        .expect("error delta");
    assert!(usage[0] < error, "{deltas:?}");
    assert!(matches!(
        deltas.last(),
        Some(ChatDelta::Done {
            stop_reason: StopReason::Aborted
        })
    ));
}

#[test]
fn chat_usage_is_sent_when_the_stream_ends_without_a_stop() {
    // Truncated stream: no `message_stop`. The caller adds its own `Done`, so
    // the usage learned so far must be out before it.
    let deltas = run_chat_against(&[MESSAGE_START, TEXT]);
    let usage = usage_positions(&deltas);
    assert_eq!(usage.len(), 1, "{deltas:?}");
    assert_eq!(usage[0], deltas.len() - 2, "{deltas:?}");
    assert!(matches!(deltas.last(), Some(ChatDelta::Done { .. })));
}
