//! Harness for the multi-document MCP end-to-end tests: three saved `.op`
//! files in two projects opened as tabs of a headless `DesktopApp`, the real
//! live MCP server on an ephemeral loopback port, and a real TCP / HTTP client
//! whose calls the test thread answers by pumping the app (`poll_mcp_server`),
//! the way the event loop does.

use crate::DesktopApp;
use op_editor_core::DocumentId;
use op_host_services::doc_io::save_to_path;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long a test waits for a set of client calls to be answered.
pub(super) const DRIVE_TIMEOUT: Duration = Duration::from_secs(60);
/// Read timeout of a client socket: above the server's own 15 s ack budget so
/// a server-side timeout is reported by the server, not by the client.
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The node every fixture document starts with (`EditorState::starter`).
pub(super) const FRAME_ID: &str = "n10";

/// One HTTP reply, parsed.
#[derive(Debug, Clone)]
pub(super) struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    /// A transport error, a JSON-RPC error or a tool result with `isError`.
    pub(super) fn is_error(&self) -> bool {
        self.status != 200
            || self.body.get("error").is_some()
            || self.body["result"]["isError"] == Value::Bool(true)
    }

    /// The text of the first content block, or the JSON-RPC error message.
    pub(super) fn text(&self) -> String {
        self.body["result"]["content"][0]["text"]
            .as_str()
            .or_else(|| self.body["error"]["message"].as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// The tool result text parsed as JSON.
    pub(super) fn json(&self) -> Value {
        serde_json::from_str(&self.text()).unwrap_or_else(|e| panic!("{e}: {}", self.text()))
    }

    pub(super) fn replayed(&self) -> bool {
        self.body["result"]["_meta"]["replayed"] == Value::Bool(true)
    }
}

/// POST one JSON-RPC `tools/call` to `path` and parse the reply. Runs on a
/// client thread; never panics, so a failure travels back as an `Err`.
pub(super) fn post_tool(
    port: u16,
    path: &str,
    tool: &str,
    arguments: Value,
) -> Result<Reply, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    })
    .to_string();
    post_raw(port, path, &body)
}

pub(super) fn post_raw(port: u16, path: &str, body: &str) -> Result<Reply, String> {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(CLIENT_READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write: {e}"))?;
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let (head, payload) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("no HTTP header in {raw:?}"))?;
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("no status in {head:?}"))?;
    let body = serde_json::from_str(payload.trim())
        .map_err(|e| format!("reply is not JSON ({e}): {payload:?}"))?;
    Ok(Reply { status, body })
}

/// A client job: runs on its own thread with the server port.
pub(super) type Job<T> = Box<dyn FnOnce(u16) -> T + Send>;

pub(super) fn canonical_temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-e2e-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    // macOS temp dirs sit behind the `/var` -> `/private/var` symlink.
    std::fs::canonicalize(dir).expect("canonical temp dir")
}

/// Two projects and three open documents: `a.op`, `b.op` in P1 and `c.op` in
/// P2. `c.op` was opened last, so it is on screen and `a`, `b` are parked.
pub(super) struct Fixture {
    pub app: DesktopApp,
    pub port: u16,
    pub root: PathBuf,
    pub p1: PathBuf,
    pub p2: PathBuf,
    pub a: DocumentId,
    pub b: DocumentId,
    pub c: DocumentId,
    pub path_a: PathBuf,
    pub path_b: PathBuf,
    pub path_c: PathBuf,
}

impl Fixture {
    pub(super) fn new(tag: &str) -> Self {
        let root = canonical_temp_dir(tag);
        let (p1, p2) = (root.join("p1"), root.join("p2"));
        for project in [&p1, &p2] {
            std::fs::create_dir_all(project).expect("project dir");
            std::fs::write(project.join("package.json"), "{}").expect("project marker");
        }
        let mut app = DesktopApp::new(None);
        let mut paths = Vec::new();
        for (dir, name) in [(&p1, "a.op"), (&p1, "b.op"), (&p2, "c.op")] {
            let path = dir.join(name);
            save_to_path(&op_editor_core::EditorState::starter(), &path).expect("fixture saves");
            assert!(app.open_document_tab(path.clone()), "{}", path.display());
            paths.push(path);
        }
        app.force_live_mcp_port = Some(0);
        assert!(app.reconcile_mcp_server_from_settings());
        let port = app.mcp_server.as_ref().expect("live MCP server").port();
        let id = |index| app.documents.id_at(index).expect("open tab");
        let (a, b, c) = (id(0), id(1), id(2));
        assert_eq!(app.documents.active_id(), c, "c is on screen");
        let mut paths = paths.into_iter();
        Self {
            path_a: paths.next().unwrap(),
            path_b: paths.next().unwrap(),
            path_c: paths.next().unwrap(),
            app,
            port,
            root,
            p1,
            p2,
            a,
            b,
            c,
        }
    }

    /// `/mcp/d/<N>` for a document.
    pub(super) fn route(id: DocumentId) -> String {
        format!("/mcp/d/{}", id.0)
    }

    /// Run client jobs on their own threads while this thread pumps the app,
    /// and return their results in order. Panics (failing the test) when a
    /// job panics or the jobs do not finish within [`DRIVE_TIMEOUT`].
    pub(super) fn drive<T: Send + 'static>(&mut self, jobs: Vec<Job<T>>) -> Vec<T> {
        self.drive_with(jobs, |app| {
            app.poll_mcp_server();
        })
    }

    /// [`Self::drive`] with the pump supplied by the test, to interpose on the
    /// UI thread's side of the exchange.
    pub(super) fn drive_with<T: Send + 'static>(
        &mut self,
        jobs: Vec<Job<T>>,
        mut pump: impl FnMut(&mut DesktopApp),
    ) -> Vec<T> {
        let port = self.port;
        let (tx, rx) = mpsc::channel();
        let count = jobs.len();
        let handles: Vec<_> = jobs
            .into_iter()
            .enumerate()
            .map(|(index, job)| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let outcome =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(port)));
                    let _ = tx.send((index, outcome));
                })
            })
            .collect();
        let mut results: Vec<Option<T>> = (0..count).map(|_| None).collect();
        let mut finished = 0;
        let deadline = Instant::now() + DRIVE_TIMEOUT;
        while finished < count {
            pump(&mut self.app);
            while let Ok((index, outcome)) = rx.try_recv() {
                match outcome {
                    Ok(value) => results[index] = Some(value),
                    Err(_) => panic!("client job {index} panicked"),
                }
                finished += 1;
            }
            assert!(
                Instant::now() < deadline,
                "client calls were not answered in {DRIVE_TIMEOUT:?} (deadlock?)"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        for handle in handles {
            let _ = handle.join();
        }
        results.into_iter().map(|r| r.expect("finished")).collect()
    }

    /// One tool call, answered by pumping.
    pub(super) fn call(&mut self, path: &str, tool: &str, arguments: Value) -> Reply {
        let (path, tool) = (path.to_string(), tool.to_string());
        let mut replies =
            self.drive(vec![
                Box::new(move |port| post_tool(port, &path, &tool, arguments))
                    as Job<Result<Reply, String>>,
            ]);
        replies.remove(0).expect("HTTP exchange")
    }

    /// Run `f` on document `id`'s state without switching tabs: the on-screen
    /// document directly, a parked one through the host's lens. Independent of
    /// the MCP resolver, so it can judge it.
    pub(super) fn peek<R>(
        &mut self,
        id: DocumentId,
        f: impl FnOnce(&op_editor_core::EditorState) -> R,
    ) -> R {
        if id == self.app.documents.active_id() {
            return f(self.app.host.editor_state());
        }
        let index = self.app.documents.index_of(id).expect("open document");
        let mut parked = self.app.documents.take_parked(index).expect("parked");
        let result = self
            .app
            .host
            .with_parked_document(&mut parked, |host| f(host.editor_state()));
        self.app.documents.restore_parked(index, parked);
        result.expect("lens ran")
    }

    /// The serialized page tree: equal strings mean an equal document.
    pub(super) fn fingerprint(&mut self, id: DocumentId) -> String {
        self.peek(id, |state| {
            serde_json::to_string(&state.doc).expect("doc json")
        })
    }

    /// The `x` of the fixture frame.
    pub(super) fn frame_x(&mut self, id: DocumentId) -> f64 {
        self.peek(id, |state| frame_field(state, "x"))
    }

    pub(super) fn frame_y(&mut self, id: DocumentId) -> f64 {
        self.peek(id, |state| frame_field(state, "y"))
    }

    /// Whether the document differs from its saved revision.
    pub(super) fn is_dirty(&mut self, id: DocumentId) -> bool {
        self.app.sync_active_slot();
        let index = self.app.documents.index_of(id).expect("open document");
        let live = self.app.document_is_dirty();
        self.app.documents.is_dirty(index, live)
    }

    /// `d<N>` for a document.
    pub(super) fn name(id: DocumentId) -> String {
        format!("d{}", id.0)
    }
}

fn frame_field(state: &op_editor_core::EditorState, key: &str) -> f64 {
    let frame = serde_json::to_value(&state.doc.children[0]).expect("frame json");
    frame[key].as_f64().unwrap_or(0.0)
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut server) = self.app.mcp_server.take() {
            server.stop();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn read_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `update_node` arguments that set the fixture frame's `x` and `y`.
pub(super) fn move_frame(x: i64, y: i64) -> Value {
    serde_json::json!({"nodeId": FRAME_ID, "data": {"x": x, "y": y}})
}
