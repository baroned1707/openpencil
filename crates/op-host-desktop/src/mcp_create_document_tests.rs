//! `create_document` end to end on a headless `DesktopApp`: a real HTTP call
//! to the live MCP server, answered by the real `poll_mcp_server` pump, then
//! the editor-side binding (path, tab, recent files, saved state).

use crate::DesktopApp;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn temp_project(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-create-doc-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp project");
    std::fs::canonicalize(dir).expect("canonical temp project")
}

fn app_with_live_server() -> (DesktopApp, u16) {
    let mut app = DesktopApp::new(None);
    app.force_live_mcp_port = Some(0);
    assert!(app.reconcile_mcp_server_from_settings());
    let port = app.mcp_server.as_ref().expect("live MCP server").port();
    (app, port)
}

/// POST one `tools/call` from a client thread while this thread pumps the
/// app, the way the event loop does. Returns the JSON-RPC reply.
fn call_tool(app: &mut DesktopApp, port: u16, tool: &str, arguments: serde_json::Value) -> String {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    })
    .to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect MCP server");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("read timeout");
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        let _ = tx.send(response);
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_mcp_server();
        if let Ok(response) = rx.try_recv() {
            return response;
        }
        assert!(Instant::now() < deadline, "the MCP call never completed");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn project_arguments(project: &Path, name: &str) -> serde_json::Value {
    serde_json::json!({"projectRoot": project.to_string_lossy(), "name": name})
}

#[test]
fn tab_label_and_path_update_after_create_document() {
    let project = temp_project("bind");
    let (mut app, port) = app_with_live_server();
    // An untitled document with unsaved edits, which the call must save.
    app.host.editor_state_mut().mark_document_changed();
    assert!(app.current_path.is_none());
    assert!(app.document_is_dirty());

    let response = call_tool(
        &mut app,
        port,
        "create_document",
        project_arguments(&project, "landing"),
    );

    let expected = project.join("designs").join("landing.op");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    let reply: serde_json::Value =
        serde_json::from_str(&op_host_services::mcp_serve::tool_text(&response)).expect("JSON");
    assert_eq!(reply["ok"], serde_json::json!(true));
    assert_eq!(reply["filePath"], expected.to_string_lossy().as_ref());
    assert!(expected.is_file());
    // The document is bound exactly as after a user Save As.
    assert_eq!(app.current_path.as_deref(), Some(expected.as_path()));
    assert!(!app.document_is_dirty(), "the saved document is clean");
    assert_eq!(
        app.host
            .editor_state()
            .editor_ui
            .file_name_display
            .as_deref(),
        Some("landing.op")
    );
    let recent = &app.host.editor_state().editor_ui.recent_files;
    assert_eq!(
        recent.first().map(|entry| entry.path.as_str()),
        expected.to_str(),
        "the new file is the most recent"
    );
    app.publish_document_tabs();
    let tabs = &app.host.editor_state().editor_ui.document_tabs;
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].label, "landing.op");
    // A file in `<project>/designs/` groups under the project, not under
    // "designs" (which every project has).
    assert_eq!(
        tabs[0].project,
        project.file_name().unwrap().to_string_lossy().as_ref()
    );
    assert!(!tabs[0].dirty);
    assert_eq!(app.documents.open_paths(), vec![expected.clone()]);

    // The file is now the open document: it can no longer be edited by
    // `filePath`, and a second create is refused as already saved.
    let by_path = call_tool(
        &mut app,
        port,
        "add_page",
        serde_json::json!({"filePath": expected.to_string_lossy(), "name": "Sneaky"}),
    );
    assert!(by_path.contains("omit filePath"), "{by_path}");
    let again = call_tool(
        &mut app,
        port,
        "create_document",
        project_arguments(&project, "second"),
    );
    assert!(again.contains("already saved at"), "{again}");
    assert!(!project.join("designs").join("second.op").exists());
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn create_document_refuses_an_existing_name_and_binds_nothing() {
    let project = temp_project("exists");
    let designs = project.join("designs");
    std::fs::create_dir_all(&designs).unwrap();
    std::fs::write(designs.join("taken.op"), "precious").unwrap();
    let (mut app, port) = app_with_live_server();

    let response = call_tool(
        &mut app,
        port,
        "create_document",
        project_arguments(&project, "taken.op"),
    );

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("never overwritten"), "{response}");
    assert_eq!(
        std::fs::read_to_string(designs.join("taken.op")).unwrap(),
        "precious"
    );
    assert!(app.current_path.is_none(), "a refusal binds nothing");
    let _ = std::fs::remove_dir_all(project);
}
