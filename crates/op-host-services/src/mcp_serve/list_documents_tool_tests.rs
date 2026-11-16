//! The headless transports (`--mcp`, `--mcp-http`) over a real loopback
//! socket: the library entry `serve_http_connection` that `--mcp-http`'s accept
//! loop runs per connection, driven by a real TCP client.

use super::*;
use crate::mcp_live::{DocumentResolver, SingleDocumentResolver};
use crate::mcp_serve::{load_editor_state, serve_http_connection, tool_text};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

/// A project with a marker holding one saved document, `<root>/designs/x.op`.
struct Project {
    root: PathBuf,
    file: PathBuf,
}

impl Project {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!(
            "openpencil-headless-list-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("designs")).expect("project dirs");
        let root = std::fs::canonicalize(root).expect("canonical root");
        std::fs::write(root.join("package.json"), "{}").expect("marker");
        let file = root.join("designs").join("x.op");
        crate::doc_io::save_to_path(&EditorState::starter(), &file).expect("fixture saves");
        Self { root, file }
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Serve `calls` connections of the `--mcp-http` library entry over `file` on
/// an ephemeral loopback port while `client` talks to it; the thread ends
/// after the last connection, so nothing is left running.
fn with_headless_server(file: &Path, calls: usize, client: impl FnOnce(u16)) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let path = file.to_path_buf();
    let server = std::thread::spawn(move || {
        let mut state = load_editor_state(&path).expect("load document");
        for _ in 0..calls {
            let (mut stream, _) = listener.accept().expect("accept");
            serve_http_connection(&mut stream, &mut state, &path).expect("serve");
        }
    });
    client(port);
    server.join().expect("server thread");
}

fn call(port: u16, tool: &str, arguments: Value) -> String {
    let body = json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name": tool, "arguments": arguments}})
    .to_string();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    response
}

fn frame_x(file: &Path) -> f64 {
    let state = load_editor_state(file).expect("reload");
    serde_json::to_value(&state.doc.children[0]).unwrap()["x"]
        .as_f64()
        .unwrap_or(0.0)
}

#[test]
fn headless_list_documents_reports_the_real_path_label_and_project() {
    let project = Project::new("real");
    with_headless_server(&project.file, 1, |port| {
        let response = call(port, "list_documents", json!({}));
        let rows: Value = serde_json::from_str(&tool_text(&response)).expect("rows");
        assert_eq!(rows.as_array().map(Vec::len), Some(1), "{response}");
        let row = &rows[0];
        assert_eq!(row["documentId"], "d1");
        assert_eq!(row["path"], project.file.to_string_lossy().as_ref());
        assert_eq!(row["label"], "x.op");
        let root_name = project.root.file_name().unwrap().to_string_lossy();
        assert_eq!(row["project"], root_name.as_ref(), "the marker's folder");
        assert_eq!(row["active"], true);
        assert_eq!(row["status"], "idle");
    });
}

#[test]
fn headless_accepts_document_id_d1_and_no_id_and_refuses_another() {
    let project = Project::new("d1");
    with_headless_server(&project.file, 5, |port| {
        let edit = |x: i64, extra: Value| {
            let mut arguments = json!({"nodeId": "n10", "data": {"x": x}});
            for (key, value) in extra.as_object().unwrap() {
                arguments[key] = value.clone();
            }
            call(port, "update_node", arguments)
        };
        // No id: every client that predates addressing.
        assert!(!edit(11, json!({})).contains("isError"));
        assert_eq!(frame_x(&project.file), 11.0);
        // d1 (with an operationId, which headless mode strips and ignores).
        let named = edit(22, json!({"documentId": "d1", "operationId": "op-9"}));
        assert!(!named.contains("isError"), "{named}");
        assert_eq!(frame_x(&project.file), 22.0);
        // Any other document is refused and the file is untouched.
        let refused = edit(33, json!({"documentId": "d2"}));
        assert!(refused.contains(r#""isError":true"#), "{refused}");
        assert!(refused.contains("exactly one document, d1"), "{refused}");
        assert_eq!(frame_x(&project.file), 22.0);
        // list_documents accepts both spellings too.
        for arguments in [json!({}), json!({"documentId": "d1"})] {
            let listed = call(port, "list_documents", arguments);
            assert!(listed.contains(r#""documentId\":\"d1\""#), "{listed}");
        }
    });
}

#[test]
fn headless_dirty_flag_after_a_write_that_was_already_saved() {
    // Every headless write is saved before it is acknowledged, so the listing
    // must not claim unsaved edits.
    let project = Project::new("dirty");
    with_headless_server(&project.file, 2, |port| {
        let write = call(
            port,
            "update_node",
            json!({"nodeId": "n10", "data": {"x": 4}}),
        );
        assert!(!write.contains("isError"), "{write}");
        let rows: Value =
            serde_json::from_str(&tool_text(&call(port, "list_documents", json!({})))).unwrap();
        assert_eq!(rows[0]["dirty"], false, "{rows}");
    });
}

#[test]
fn single_document_resolver_lists_its_file_and_project() {
    let project = Project::new("resolver");
    let mut state = EditorState::new();
    let mut resolver = SingleDocumentResolver::new(&mut state);
    resolver.path = Some(project.file.clone());
    let listing = resolver.documents().remove(0);
    assert_eq!(listing.label, "x.op");
    assert_eq!(listing.path.as_deref(), Some(project.file.as_path()));
    assert_eq!(
        listing.project,
        project.root.file_name().unwrap().to_string_lossy()
    );
    let mut untitled_state = EditorState::new();
    let untitled = SingleDocumentResolver::new(&mut untitled_state)
        .documents()
        .remove(0);
    assert_eq!(
        (untitled.label.as_str(), untitled.project.as_str()),
        ("Untitled", "")
    );
    assert_eq!(untitled.path, None);
}

#[test]
fn headless_catalog_does_not_advertise_the_live_addressing_arguments() {
    // The live server adds `documentId` / `operationId` to its catalog; the
    // single-document headless modes accept `d1` but do not need to advertise it.
    let project = Project::new("catalog");
    with_headless_server(&project.file, 1, |port| {
        let body = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        assert!(response.contains("update_node"), "a real catalog came back");
        // The `list_documents` description names the id; what must be absent
        // is the schema PROPERTY the live server adds to every tool.
        assert!(
            !response.contains(r#""documentId":{"#),
            "headless schemas are unchanged"
        );
        assert!(!response.contains(r#""operationId":{"#));
        assert!(!response.contains("x-mcp-header"));
    });
}
