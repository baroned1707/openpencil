//! `create_document` through the real `serve_connection` router with a stub
//! UI thread feeding a real `McpLiveServer::pump`, so the connection-side
//! validation, the epoch check and the UI-side write are all the production
//! code. Directories live under the OS temp dir; the home directory is the
//! machine's real one but is only ever compared against, never written.

use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

static TREE_SEQ: AtomicU32 = AtomicU32::new(0);

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-create-document-route-{}-{}",
            std::process::id(),
            TREE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// What the stub UI thread ended with.
struct UiOutcome {
    state: EditorState,
    created: Vec<PathBuf>,
}

/// POST `body` to the router. The UI side shares `open_paths` with the
/// connection side exactly as the production server does, and `after_snapshot`
/// runs on the UI thread between the call's snapshot and its create request.
fn call(
    body: &str,
    open_paths: &OpenDocumentPaths,
    after_snapshot: impl FnOnce(&mut EditorState) + Send + 'static,
) -> (String, UiOutcome) {
    call_at("/mcp", body, open_paths, after_snapshot)
}

fn call_at(
    route: &str,
    body: &str,
    open_paths: &OpenDocumentPaths,
    after_snapshot: impl FnOnce(&mut EditorState) + Send + 'static,
) -> (String, UiOutcome) {
    let (conn_tx, conn_rx) = mpsc::channel::<UiRequest>();
    let ui_paths = open_paths.clone();
    let ui = thread::spawn(move || {
        let (req_tx, req_rx) = mpsc::channel();
        let (stop_tx, _stop_rx) = mpsc::channel();
        let mut server = McpLiveServer {
            port: 0,
            token: "test-token".to_string(),
            quit_flag: Arc::new(AtomicBool::new(false)),
            req_rx,
            stop_tx,
            client_identity: Arc::new(Mutex::new(None)),
            last_mcp_epoch: 0,
            pending_design_md: VecDeque::new(),
            host_document_epoch: 0,
            open_paths: ui_paths,
        };
        let mut state = EditorState::starter();
        state.mark_document_changed();
        let mut hook = Some(after_snapshot);
        let mut created = Vec::new();
        while let Ok(request) = conn_rx.recv_timeout(Duration::from_secs(5)) {
            let was_snapshot = matches!(request, UiRequest::Snapshot { .. });
            req_tx.send(request).expect("forward to the pump");
            created.extend(server.pump(&mut state).created_document);
            if was_snapshot {
                if let Some(hook) = hook.take() {
                    hook(&mut state);
                }
            }
        }
        UiOutcome { state, created }
    });
    let admission = LiveAdmission::new("tok".to_string(), 51237);
    let stateful_lock = Mutex::new(());
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let request = format!(
        "POST {route} HTTP/1.1\r\nHost: 127.0.0.1:51237\r\nContent-Length: {}\r\n\r\n{body}",
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
    let outcome = ui.join().expect("ui thread");
    (
        String::from_utf8_lossy(&stream.output).into_owned(),
        outcome,
    )
}

fn create_call(project_root: &Path, name: &str) -> String {
    let arguments =
        serde_json::json!({"projectRoot": project_root.to_string_lossy(), "name": name});
    format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{{"name":"create_document","arguments":{arguments}}}}}"#
    )
}

#[test]
fn create_document_writes_under_designs_and_binds_the_editor() {
    let tree = Tree::new();
    let open_paths = OpenDocumentPaths::default();

    let (response, ui) = call(&create_call(&tree.0, "landing"), &open_paths, |_| {});

    let expected = tree.0.join("designs").join("landing.op");
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    let reply: serde_json::Value =
        serde_json::from_str(&crate::mcp_serve::tool_text(&response)).expect("result JSON");
    assert_eq!(reply["ok"], serde_json::json!(true));
    assert_eq!(reply["filePath"], expected.to_string_lossy().as_ref());
    assert_eq!(reply["projectRoot"], tree.0.to_string_lossy().as_ref());
    assert_eq!(reply["name"], "landing");
    // The file holds the live document...
    let written = crate::doc_io::load_editor_state(&expected, op_editor_core::Locale::EnUs)
        .expect("written file loads");
    assert_eq!(written.doc, ui.state.doc);
    // ...the pump asked the host to bind it, and marked the document clean
    // in the same step as the write.
    assert_eq!(ui.created, vec![expected.clone()]);
    assert!(!ui.state.is_dirty(), "the saved document is clean");
    assert_eq!(
        ui.state.editor_ui.file_name_display.as_deref(),
        Some("landing.op")
    );
    // A follow-up call naming the file is refused before the host's next sync.
    assert_eq!(open_paths.active(), Some(expected.clone()));
    assert!(open_paths.snapshot().contains(&expected));
    let follow_up = format!(
        r#"{{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{{"name":"add_page","arguments":{{"filePath":{},"name":"X"}}}}}}"#,
        serde_json::to_string(&expected.to_string_lossy()).unwrap()
    );
    let (refused, _) = call(&follow_up, &open_paths, |_| {
        panic!("a refused filePath must not reach the UI thread");
    });
    assert!(refused.contains("omit filePath"), "{refused}");
}

#[test]
fn create_document_refuses_when_document_already_has_a_file() {
    let tree = Tree::new();
    let saved = tree.0.join("already.op");
    let open_paths = OpenDocumentPaths::default();
    open_paths.set(std::slice::from_ref(&saved));
    open_paths.set_active(Some(&saved));

    let (response, ui) = call(&create_call(&tree.0, "second"), &open_paths, |_| {
        panic!("a refused call must not reach the UI thread");
    });

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("already saved at"), "{response}");
    assert!(response.contains("save_document"), "{response}");
    assert!(
        response.contains(&saved.display().to_string()),
        "{response}"
    );
    assert!(!tree.0.join("designs").exists());
    assert!(ui.created.is_empty());
}

#[test]
fn the_ui_thread_rechecks_that_the_document_is_untitled() {
    // The connection-side check can be stale by the time the UI thread runs:
    // the host may have bound a file in between. The UI answer is final.
    let tree = Tree::new();
    let open_paths = OpenDocumentPaths::default();
    let saved = tree.0.join("bound-meanwhile.op");
    let ui_paths = open_paths.clone();

    let (response, ui) = call(&create_call(&tree.0, "late"), &open_paths, move |_| {
        ui_paths.set_active(Some(&saved));
    });

    assert!(response.contains("already saved at"), "{response}");
    assert!(!tree.0.join("designs").exists());
    assert!(ui.created.is_empty());
}

#[test]
fn create_document_is_refused_after_the_document_changed_mid_call() {
    let tree = Tree::new();
    let open_paths = OpenDocumentPaths::default();

    let (response, ui) = call(&create_call(&tree.0, "stale"), &open_paths, |state| {
        state.replace_document(EditorState::new().doc);
    });

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response
            .contains("the open document changed while this call was running; re-read and retry"),
        "{response}"
    );
    assert!(!tree.0.join("designs").exists(), "nothing may be written");
    assert!(ui.created.is_empty());
    assert_eq!(open_paths.active(), None);
}

#[test]
fn bad_arguments_are_refused_without_waking_the_ui_thread() {
    let tree = Tree::new();
    let open_paths = OpenDocumentPaths::default();
    for (body, expected) in [
        (create_call(&tree.0, "../escape"), "name is not a usable file name"),
        (create_call(Path::new("relative/dir"), "x"), "must be an absolute path"),
        (
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"create_document","arguments":{"name":"x"}}}"#.to_string(),
            "projectRoot is required",
        ),
    ] {
        let (response, ui) = call(&body, &open_paths, |_| {
            panic!("a refused call must not reach the UI thread");
        });
        assert!(response.contains(r#""isError":true"#), "{response}");
        assert!(response.contains(expected), "{response}");
        assert!(ui.created.is_empty());
    }
}

#[test]
fn the_lean_route_still_refuses_create_document_as_not_in_profile() {
    let tree = Tree::new();

    let (response, ui) = call_at(
        "/mcp/lean",
        &create_call(&tree.0, "x"),
        &OpenDocumentPaths::default(),
        |_| {},
    );

    assert!(response.contains("tool-not-in-profile"), "{response}");
    assert!(!tree.0.join("designs").exists());
    assert!(ui.created.is_empty());
}
