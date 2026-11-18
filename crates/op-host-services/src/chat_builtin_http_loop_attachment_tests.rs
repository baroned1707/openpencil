//! A tool-loop turn carries the user's reference images natively: the first
//! user message holds the image block, and the prompt text carries no
//! `[attached image: <local path>]` line a remote model cannot open.

use super::*;
use op_ai::chat_provider::{ChatAttachment, ChatToolResult};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Instant;

struct NoTools;
impl ChatToolExecutor for NoTools {
    fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
        panic!("the scripted reply calls no tool")
    }
}

fn reference() -> ChatAttachment {
    ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![1, 2, 3, 4],
    }
}

fn loop_provider(
    kind: BuiltinAgentKind,
    model: &str,
    base_url: String,
) -> ConfiguredBuiltinProvider {
    let config = BuiltinAgentConfig {
        id: "loop-attachment-test".into(),
        preset: op_editor_core::BuiltinAgentPresetKey::Custom,
        display_name: "Loop attachment test".into(),
        kind,
        api_key: "sk-test".into(),
        models: vec![model.into()],
        base_url,
        enabled: true,
    };
    let mut provider = ConfiguredBuiltinProvider::from_builtin_agent(&config).unwrap();
    provider.http_client = Some(
        reqwest::Client::builder()
            .use_rustls_tls()
            .no_proxy()
            .build()
            .unwrap(),
    );
    provider.min_gap = Duration::ZERO;
    provider.max_retries = 0;
    provider.with_canvas_tools(
        vec![ChatToolDef {
            name: "get_editor_state".into(),
            description: "read".into(),
            level: "read".into(),
            input_schema_json: r#"{"type":"object"}"#.into(),
        }],
        Arc::new(NoTools),
    )
}

/// One-shot loopback server: answers a plain text SSE reply and hands back
/// the JSON request body it received.
fn capture_server(kind: BuiltinAgentKind) -> (String, std::thread::JoinHandle<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let payload = match kind {
        BuiltinAgentKind::OpenAiCompat => format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices": [{"delta": {"content": "ok"}, "finish_reason": "stop"}]})
        ),
        BuiltinAgentKind::Anthropic => format!(
            "data: {}\n\ndata: {}\n\n",
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ok"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}})
        ),
    };
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "provider never connected");
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("accept failed: {error}"),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut head = Vec::new();
        let mut byte = [0_u8];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        let length = String::from_utf8_lossy(&head)
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("content length");
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream.write_all(response.as_bytes()).unwrap();
        serde_json::from_slice(&body).unwrap()
    });
    (format!("http://{address}"), server)
}

#[test]
fn attachments_reach_the_anthropic_and_openai_loops() {
    for (kind, model) in [
        (BuiltinAgentKind::Anthropic, "claude-vision-test"),
        (BuiltinAgentKind::OpenAiCompat, "gpt-vision-test"),
    ] {
        let (base_url, server) = capture_server(kind.clone());
        let provider = loop_provider(kind.clone(), model, base_url);
        let deltas: Vec<ChatDelta> = provider
            .send(ChatRequest {
                system_prompt: "You are a design agent.".into(),
                user_message: "match the reference".into(),
                attachments: vec![reference()],
                ..Default::default()
            })
            .collect();
        assert!(
            !deltas.iter().any(|d| matches!(d, ChatDelta::Error(_))),
            "{deltas:?}"
        );
        let body = server.join().expect("request captured");
        let first_user = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .expect("a user message");
        let content = &first_user["content"];
        let blocks = content.as_array().expect("multimodal first user message");
        assert_eq!(blocks[0]["text"], "match the reference");
        match kind {
            BuiltinAgentKind::Anthropic => assert_eq!(blocks[1]["source"]["data"], "AQIDBA=="),
            BuiltinAgentKind::OpenAiCompat => assert_eq!(
                blocks[1]["image_url"]["url"],
                "data:image/png;base64,AQIDBA=="
            ),
        }
        // No spilled-file path line next to the real image block.
        assert!(!body.to_string().contains("[attached"), "{body}");
    }
}

#[test]
fn a_text_only_model_gets_the_loops_attachment_error_before_any_request() {
    // Nothing listens on this port: a request would surface as a transport
    // error instead of the attachment error.
    let provider = loop_provider(
        BuiltinAgentKind::OpenAiCompat,
        "deepseek-chat",
        "http://127.0.0.1:9".into(),
    );
    let deltas: Vec<ChatDelta> = provider
        .send(ChatRequest {
            user_message: "match the reference".into(),
            attachments: vec![reference()],
            ..Default::default()
        })
        .collect();
    let message = deltas
        .iter()
        .find_map(|d| match d {
            ChatDelta::Error(message) => Some(message.clone()),
            _ => None,
        })
        .expect("an error delta");
    assert!(
        message.contains("does not support reference images"),
        "{message}"
    );
    assert!(matches!(
        deltas.last(),
        Some(ChatDelta::Done {
            stop_reason: StopReason::Aborted
        })
    ));
}
