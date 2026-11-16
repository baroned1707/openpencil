//! Shared harness for the live-server tests: a multi-document stand-in for the
//! host, a UI thread that pumps a REAL `McpLiveServer` over it, and a driver
//! that posts one request through the real `serve_connection` router.

use super::*;
use std::path::PathBuf;

pub(super) const TEST_PORT: u16 = 51240;

/// Document `number` as this launch spells it, `d<number>.<nonce>`: the only
/// form a live server accepts while several documents are open.
pub(super) fn full(number: u64) -> String {
    crate::mcp_serve::document_address::format_spoken_id(
        DocumentId(number),
        Some(crate::mcp_serve::document_nonce::launch_nonce()),
    )
}

/// `"documentId":"d<number>.<nonce>"`, the argument fragment of a tool call.
pub(super) fn document_arg(number: u64) -> String {
    format!(r#""documentId":"{}""#, full(number))
}

pub(super) struct MockStream {
    input: std::io::Cursor<Vec<u8>>,
    pub(super) output: Vec<u8>,
}

impl MockStream {
    pub(super) fn new(request: String) -> Self {
        Self {
            input: std::io::Cursor::new(request.into_bytes()),
            output: Vec::new(),
        }
    }

    pub(super) fn response(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }
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

/// One open document of the stand-in host.
pub(super) struct TestDocument {
    pub(super) id: DocumentId,
    pub(super) label: String,
    pub(super) path: Option<PathBuf>,
    pub(super) state: EditorState,
    pub(super) host_epoch: u64,
    pub(super) active: bool,
    pub(super) status: op_editor_core::DocumentTabStatus,
}

/// A host with several documents; only the active one is reachable, like the
/// desktop's parked documents, unless `lend_parked` says otherwise.
pub(super) struct TestResolver {
    pub(super) documents: Vec<TestDocument>,
    pub(super) lend_parked: bool,
}

impl TestResolver {
    /// One untitled document, `d1`, on screen.
    pub(super) fn single(state: EditorState) -> Self {
        Self {
            documents: vec![TestDocument {
                id: DocumentId(1),
                label: "Untitled".to_string(),
                path: None,
                state,
                host_epoch: 0,
                active: true,
                status: op_editor_core::DocumentTabStatus::Idle,
            }],
            lend_parked: false,
        }
    }

    /// Documents `d<n>` for each `(n, active)` pair, each with a fresh state.
    pub(super) fn with_ids(ids: &[(u64, bool)]) -> Self {
        Self {
            documents: ids
                .iter()
                .map(|&(id, active)| TestDocument {
                    id: DocumentId(id),
                    label: format!("doc{id}.op"),
                    path: Some(PathBuf::from(format!("/work/proj/doc{id}.op"))),
                    state: EditorState::new(),
                    host_epoch: 0,
                    active,
                    status: op_editor_core::DocumentTabStatus::Idle,
                })
                .collect(),
            lend_parked: false,
        }
    }

    pub(super) fn document_mut(&mut self, id: u64) -> &mut TestDocument {
        self.documents
            .iter_mut()
            .find(|document| document.id == DocumentId(id))
            .expect("test document")
    }

    /// What the desktop does after a pump that created a file: bind the path
    /// to the document on screen.
    pub(super) fn bind_created(&mut self, path: PathBuf) {
        if let Some(active) = self.documents.iter_mut().find(|document| document.active) {
            active.label = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            active.path = Some(path);
        }
    }
}

impl DocumentResolver for TestResolver {
    fn documents(&self) -> Vec<DocumentListing> {
        self.documents
            .iter()
            .map(|document| DocumentListing {
                id: document.id,
                label: document.label.clone(),
                path: document.path.clone(),
                project: "proj".to_string(),
                dirty: document.state.is_dirty(),
                active: document.active,
                status: document.status,
                nonce: None,
            })
            .collect()
    }

    fn with_document(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<(), ResolveError> {
        let document = self
            .documents
            .iter_mut()
            .find(|document| document.id == id)
            .ok_or(ResolveError::Unknown)?;
        if !document.active && !self.lend_parked {
            return Err(ResolveError::NotReachable);
        }
        f(&mut document.state, document.host_epoch);
        Ok(())
    }
}

/// A server with a queue the test feeds by hand (no listener, no threads).
pub(super) fn server_with_queue() -> (McpLiveServer, Sender<UiRequest>) {
    server_with_timing(LiveTiming::system())
}

/// [`server_with_queue`] on the given clock and budgets.
pub(super) fn server_with_timing(timing: LiveTiming) -> (McpLiveServer, Sender<UiRequest>) {
    let (req_tx, req_rx) = mpsc::channel();
    let (stop_tx, _stop_rx) = mpsc::channel();
    let server = McpLiveServer {
        port: 0,
        admission: Arc::new(LiveAdmission::new("test-token".to_string(), 0)),
        nonce: crate::mcp_serve::document_nonce::launch_nonce(),
        token: "test-token".to_string(),
        quit_flag: Arc::new(AtomicBool::new(false)),
        req_rx,
        stop_tx,
        client_identity: Arc::new(Mutex::new(None)),
        last_mcp_epoch: 0,
        pending_design_md: VecDeque::new(),
        timing,
        expired_requests: 0,
    };
    (server, req_tx)
}

/// Runs on the UI thread right after the first snapshot request was served.
pub(super) type AfterSnapshot = Box<dyn FnOnce(&mut TestResolver) + Send>;

/// What the stub UI thread ended with.
pub(super) struct UiRun {
    pub(super) resolver: TestResolver,
    pub(super) created: Vec<PathBuf>,
    /// The kind of every request the connection made, in order.
    pub(super) seen: Vec<&'static str>,
}

fn kind(request: &UiRequest) -> &'static str {
    match request {
        UiRequest::Documents { .. } => "documents",
        UiRequest::Snapshot { .. } => "snapshot",
        UiRequest::ListPages { .. } => "list_pages",
        UiRequest::Apply { .. } => "apply",
        UiRequest::ReplaceDocument { .. } => "replace",
        UiRequest::UpdateEditorMeta { .. } => "meta",
        UiRequest::GenerateDesignMd { .. } => "design_md",
        UiRequest::CreateDocument { .. } => "create",
        #[cfg(feature = "mcp-debug-tools")]
        UiRequest::Screenshot { .. } => "screenshot",
    }
}

/// A UI thread that pumps a real server over `resolver` until the connection
/// side goes quiet.
pub(super) fn spawn_ui(
    conn_rx: Receiver<UiRequest>,
    mut resolver: TestResolver,
    after_snapshot: Option<AfterSnapshot>,
) -> thread::JoinHandle<UiRun> {
    thread::spawn(move || {
        let (mut server, server_tx) = server_with_queue();
        let mut hook = after_snapshot;
        let mut created = Vec::new();
        let mut seen = Vec::new();
        while let Ok(request) = conn_rx.recv_timeout(Duration::from_secs(5)) {
            let name = kind(&request);
            seen.push(name);
            server_tx.send(request).expect("forward to the pump");
            let outcome = server.pump(&mut resolver);
            if let Some(path) = outcome.created_document {
                created.push(path.clone());
                resolver.bind_created(path);
            }
            if name == "snapshot" {
                if let Some(hook) = hook.take() {
                    hook(&mut resolver);
                }
            }
        }
        UiRun {
            resolver,
            created,
            seen,
        }
    })
}

pub(super) struct Posted {
    pub(super) response: String,
    pub(super) ui: UiRun,
}

/// One HTTP request through `serve_connection`, answered by a stub UI thread.
pub(super) fn post(
    path: &str,
    extra_headers: &str,
    body: &str,
    documents: &LiveDocuments,
    resolver: TestResolver,
    after_snapshot: Option<AfterSnapshot>,
) -> Posted {
    let (conn_tx, conn_rx) = mpsc::channel::<UiRequest>();
    let ui = spawn_ui(conn_rx, resolver, after_snapshot);
    let response = serve_one(path, extra_headers, body, documents, &conn_tx);
    drop(conn_tx);
    Posted {
        response,
        ui: ui.join().expect("ui thread"),
    }
}

/// The connection half only: post through the router with the given queue.
pub(super) fn serve_one(
    path: &str,
    extra_headers: &str,
    body: &str,
    documents: &LiveDocuments,
    req_tx: &Sender<UiRequest>,
) -> String {
    serve_one_via(
        path,
        extra_headers,
        body,
        documents,
        &UiSender::from(req_tx.clone()),
    )
}

/// [`serve_one`] over a queue that carries its own clock and budgets.
pub(super) fn serve_one_via(
    path: &str,
    extra_headers: &str,
    body: &str,
    documents: &LiveDocuments,
    req_tx: &UiSender,
) -> String {
    let admission = LiveAdmission::new("tok".to_string(), TEST_PORT);
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{TEST_PORT}\r\n{extra_headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = MockStream::new(request);
    serve_connection(
        &mut stream,
        req_tx,
        &admission,
        documents,
        &quit_flag,
        &wake_ui,
        &client_identity,
    )
    .expect("served");
    stream.response()
}

/// A clock the test moves by hand: real time at creation plus an offset.
#[derive(Clone)]
pub(super) struct TestClock {
    base: std::time::Instant,
    offset_millis: Arc<AtomicU64>,
}

impl TestClock {
    pub(super) fn new() -> Self {
        Self {
            base: std::time::Instant::now(),
            offset_millis: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(super) fn advance(&self, by: Duration) {
        self.offset_millis
            .fetch_add(by.as_millis() as u64, Ordering::SeqCst);
    }

    /// Timing on this clock with a 5 s budget and the production 2 s grace,
    /// none of which a test ever sleeps through.
    pub(super) fn timing(&self) -> LiveTiming {
        LiveTiming::new(
            Arc::new(self.clone()),
            Duration::from_secs(5),
            live_timing::STARTED_GRACE,
        )
    }
}

impl Clock for TestClock {
    fn now(&self) -> std::time::Instant {
        self.base + Duration::from_millis(self.offset_millis.load(Ordering::SeqCst))
    }
}

/// The JSON body of a raw HTTP response.
pub(super) fn json_body(response: &str) -> serde_json::Value {
    let body = response.split_once("\r\n\r\n").expect("http body").1;
    serde_json::from_str(body).expect("json body")
}

pub(super) fn is_error(response: &str) -> bool {
    json_body(response)["result"]["isError"] == serde_json::json!(true)
}

pub(super) fn error_text(response: &str) -> String {
    json_body(response)["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// A `tools/call` body.
pub(super) fn tool_call(id: i64, tool: &str, arguments: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{{{arguments}}}}}}}"#
    )
}
