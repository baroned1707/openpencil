//! The headless MCP server and a per-call `filePath` open an empty `.op` file
//! as a blank document instead of failing.

use super::*;
use crate::doc_io;

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-mcp-empty-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn tool_line(id: u32, tool: &str, arguments: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}
    })
    .to_string()
}

#[test]
fn headless_mcp_serves_a_blank_document_for_an_empty_file() {
    let dir = scratch_dir("headless");
    let path = dir.join("blank.op");
    std::fs::write(&path, "").unwrap();

    // The startup load of `--mcp <path>` / `--mcp-http`.
    let mut state = load_or_initialize_editor_state(&path).expect("an empty file opens");

    assert_eq!(state.active_children().len(), 1, "the blank starter frame");
    let formatted = std::fs::read(&path).unwrap();
    assert!(!doc_io::bytes_are_blank(&formatted), "formatted on disk");
    let opened = tool_line(1, "open_document", serde_json::json!({}));
    let reply = process_message(&mut state, &path, &opened)
        .expect("dispatch")
        .expect("reply");
    assert!(!reply.contains(r#""isError":true"#), "{reply}");
    // The first write saves over the formatted file and keeps it valid.
    let write = tool_line(
        2,
        "batch_design",
        serde_json::json!({"operations": "I(null,{\"type\":\"rectangle\",\"name\":\"R\",\"x\":0,\"y\":0,\"width\":10,\"height\":10})"}),
    );
    let reply = process_message(&mut state, &path, &write)
        .expect("dispatch")
        .expect("reply");
    assert!(!reply.contains(r#""isError":true"#), "{reply}");
    let reloaded = load_editor_state(&path).expect("valid after the write");
    assert_eq!(reloaded.active_children().len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn per_call_file_path_initializes_an_empty_file_and_leaves_bad_files_alone() {
    let dir = scratch_dir("file-path");
    let empty = dir.join("empty.op");
    let garbage = dir.join("garbage.op");
    std::fs::write(&empty, "  \n").unwrap();
    std::fs::write(&garbage, "not a document").unwrap();
    let line = |target: &std::path::Path| {
        tool_line(
            7,
            "open_document",
            serde_json::json!({"filePath": target.to_string_lossy()}),
        )
    };

    let reply = file_path::process_message_for_file_path_arg(
        None,
        &[],
        &line(&empty),
        tool_profile::McpAccessProfile::UNRESTRICTED,
    )
    .expect("dispatch")
    .expect("reply");
    assert!(!reply.contains(r#""isError":true"#), "{reply}");
    assert!(!doc_io::bytes_are_blank(&std::fs::read(&empty).unwrap()));

    let refused = file_path::process_message_for_file_path_arg(
        None,
        &[],
        &line(&garbage),
        tool_profile::McpAccessProfile::UNRESTRICTED,
    );
    assert!(refused.is_err(), "still the loader's error");
    assert_eq!(std::fs::read(&garbage).unwrap(), b"not a document");
    let _ = std::fs::remove_dir_all(dir);
}
