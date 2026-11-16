//! A live tool call is a snapshot followed by an apply. The apply must be
//! refused when the on-screen document was replaced in between (node ids
//! repeat across documents), and must still land after ordinary edits.

use super::*;

fn server_with_queue() -> (McpLiveServer, Sender<UiRequest>) {
    let (req_tx, req_rx) = mpsc::channel();
    let (stop_tx, _stop_rx) = mpsc::channel();
    let server = McpLiveServer {
        port: 0,
        token: "test-token".to_string(),
        quit_flag: Arc::new(AtomicBool::new(false)),
        req_rx,
        stop_tx,
        client_identity: Arc::new(Mutex::new(None)),
        last_mcp_epoch: 0,
        pending_design_md: VecDeque::new(),
        host_document_epoch: 0,
        open_paths: OpenDocumentPaths::default(),
    };
    (server, req_tx)
}

fn take_snapshot(
    server: &mut McpLiveServer,
    req_tx: &Sender<UiRequest>,
    state: &mut EditorState,
) -> DocumentEpoch {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Snapshot { ack: ack_tx })
        .expect("queue snapshot");
    server.pump(state);
    ack_rx.try_recv().expect("snapshot ack").epoch
}

fn apply_insert(
    server: &mut McpLiveServer,
    req_tx: &Sender<UiRequest>,
    state: &mut EditorState,
    expected_epoch: Option<DocumentEpoch>,
) -> ApplyAck {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Apply {
            tool_name: "insert_node".to_string(),
            cmd: EditorCommand::InsertNode {
                kind: "rect".to_string(),
                name: "Box".to_string(),
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                fill_hex: None,
                target_parent: op_editor_core::NodeId::NONE,
                page_id: None,
            },
            expected_epoch,
            ack: ack_tx,
        })
        .expect("queue apply");
    server.pump(state);
    ack_rx.try_recv().expect("apply ack")
}

fn node_count(state: &EditorState) -> usize {
    state.active_children().len()
}

#[test]
fn apply_rejected_after_document_epoch_change() {
    let (mut server, req_tx) = server_with_queue();
    let mut state = EditorState::new();
    let epoch = take_snapshot(&mut server, &req_tx, &mut state);

    // The document is replaced between the two halves of the call (whole-
    // document sync through the pump, or Open/New through the host).
    state.replace_document(EditorState::new().doc);
    let nodes_before = node_count(&state);
    let ack = apply_insert(&mut server, &req_tx, &mut state, Some(epoch));
    assert!(!ack.applied && ack.document_changed);
    assert_eq!(node_count(&state), nodes_before, "nothing may be applied");

    // Open/New swaps the whole `EditorState` (generation restarts at 0), so
    // the host's own epoch has to be part of the identity too.
    let mut fresh = EditorState::new();
    let epoch = take_snapshot(&mut server, &req_tx, &mut fresh);
    server.sync_host(1, &[], None);
    let ack = apply_insert(&mut server, &req_tx, &mut fresh, Some(epoch));
    assert!(!ack.applied && ack.document_changed);
    assert_eq!(node_count(&fresh), 0);
}

#[test]
fn apply_accepted_when_epoch_unchanged_by_ordinary_edit() {
    let (mut server, req_tx) = server_with_queue();
    let mut state = EditorState::new();
    let epoch = take_snapshot(&mut server, &req_tx, &mut state);

    // Ordinary edits (a user drag, an earlier tool call) change the revision
    // but not the document's identity.
    let first = apply_insert(&mut server, &req_tx, &mut state, None);
    assert!(first.applied);
    let revision = state.document_revision();
    let ack = apply_insert(&mut server, &req_tx, &mut state, Some(epoch));

    assert!(ack.applied && !ack.document_changed);
    assert_ne!(state.document_revision(), revision, "the edit landed");
    assert_eq!(node_count(&state), 2);
}

#[test]
fn apply_without_a_snapshot_is_never_epoch_checked() {
    let (mut server, req_tx) = server_with_queue();
    let mut state = EditorState::new();
    server.sync_host(7, &[], None);
    state.replace_document(EditorState::new().doc);

    let ack = apply_insert(&mut server, &req_tx, &mut state, None);

    assert!(ack.applied);
}

struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl std::io::Read for MockStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.input, buf)
    }
}

impl std::io::Write for MockStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// POST one `tools/call` to the real `serve_connection` router while a stub
/// UI thread feeds a real `McpLiveServer::pump`. `after_snapshot` runs on the
/// UI thread between the call's snapshot and its apply.
fn call_with_ui_hook(
    body: &str,
    open_paths: &OpenDocumentPaths,
    after_snapshot: impl FnOnce(&mut EditorState) + Send + 'static,
) -> String {
    let (conn_tx, conn_rx) = mpsc::channel::<UiRequest>();
    let ui = thread::spawn(move || {
        let (mut server, server_tx) = server_with_queue();
        let mut state = EditorState::new();
        let mut hook = Some(after_snapshot);
        while let Ok(request) = conn_rx.recv_timeout(Duration::from_secs(5)) {
            let was_snapshot = matches!(request, UiRequest::Snapshot { .. });
            server_tx.send(request).expect("forward to the pump");
            server.pump(&mut state);
            if was_snapshot {
                if let Some(hook) = hook.take() {
                    hook(&mut state);
                }
            }
        }
    });
    let admission = LiveAdmission::new("tok".to_string(), 51236);
    let stateful_lock = Mutex::new(());
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:51236\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = MockStream {
        input: std::io::Cursor::new(request.into_bytes()),
        output: Vec::new(),
    };
    serve_connection(
        &mut stream,
        &conn_tx,
        &admission,
        &stateful_lock,
        &quit_flag,
        &wake_ui,
        &client_identity,
        open_paths,
    )
    .expect("served");
    drop(conn_tx);
    ui.join().expect("ui thread");
    String::from_utf8_lossy(&stream.output).into_owned()
}

const CLEAR_SELECTION: &str = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"clear_selection","arguments":{}}}"#;

#[test]
fn stale_apply_reaches_the_client_as_a_tool_error_naming_the_cause() {
    let response = call_with_ui_hook(CLEAR_SELECTION, &OpenDocumentPaths::default(), |state| {
        state.replace_document(EditorState::new().doc);
    });

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response
            .contains("the open document changed while this call was running; re-read and retry"),
        "{response}"
    );
    assert!(response.contains(r#""id":9"#), "{response}");
    assert!(!response.contains("host rejected command"), "{response}");
}

#[test]
fn unchanged_document_apply_succeeds_through_the_router() {
    let response = call_with_ui_hook(CLEAR_SELECTION, &OpenDocumentPaths::default(), |_| {});

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains(r#"\"wrote\":\"true\""#), "{response}");
}

#[test]
fn router_refuses_file_path_naming_the_open_document_without_asking_the_ui() {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-router-open-document-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let open = dir.join("open.op");
    let original = r#"{"version":"1.0.0","children":[]}"#;
    std::fs::write(&open, original).expect("write doc");
    let open_paths = OpenDocumentPaths::default();
    open_paths.set(std::slice::from_ref(&open));
    let target = serde_json::to_string(&open.to_string_lossy()).expect("path json");
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"add_page","arguments":{{"filePath":{target},"name":"FromAgent"}}}}}}"#
    );

    let response = call_with_ui_hook(&body, &open_paths, |_| {
        panic!("a refused filePath must not reach the UI thread");
    });

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("omit filePath"), "{response}");
    assert_eq!(std::fs::read_to_string(&open).expect("read"), original);
    let _ = std::fs::remove_dir_all(dir);
}
