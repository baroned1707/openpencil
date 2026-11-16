//! The live server's `/mcp/lean` endpoint, driven through the real
//! `serve_connection` router with a stub UI thread.

use super::test_support::*;
use super::*;

/// POST `body` to `path` with a stub UI thread over one untitled document.
/// Returns the raw HTTP response and the UI requests the router made, by kind.
fn post(path: &str, body: &str) -> (String, Vec<&'static str>) {
    let posted = super::test_support::post(
        path,
        "",
        body,
        &LiveDocuments::default(),
        TestResolver::single(EditorState::new()),
        None,
    );
    (posted.response, posted.ui.seen)
}

fn tool_names(response: &str) -> Vec<String> {
    let body = response.split_once("\r\n\r\n").expect("http body").1;
    let value: serde_json::Value = serde_json::from_str(body).expect("json-rpc body");
    value["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name").to_string())
        .collect()
}

const TOOLS_LIST: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

#[test]
fn the_lean_path_lists_six_tools_and_the_default_path_the_full_catalog() {
    let (lean, _) = post("/mcp/lean", TOOLS_LIST);
    assert_eq!(
        tool_names(&lean),
        crate::mcp_serve::tool_catalog::LEAN_TOOLS,
        "{lean}"
    );
    let (full, _) = post("/mcp", TOOLS_LIST);
    assert!(tool_names(&full).len() > 100, "{full}");
}

#[test]
fn the_lean_path_refuses_fast_path_tools_instead_of_serving_them() {
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_pages","arguments":{}}}"#;
    let (response, seen) = post("/mcp/lean", call);
    assert!(response.contains("tool-not-in-profile"), "{response}");
    assert!(
        !seen.contains(&"list_pages") && !seen.contains(&"apply"),
        "a refused call must not reach the UI thread's tool paths: {seen:?}"
    );
}

#[test]
fn the_lean_path_serves_its_own_tools() {
    let call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_editor_state","arguments":{}}}"#;
    let (response, seen) = post("/mcp/lean", call);
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("variables"), "{response}");
    assert_eq!(seen, ["documents", "snapshot"]);
}

#[test]
fn unknown_mcp_subpaths_are_not_found() {
    let (response, seen) = post("/mcp/tiny", TOOLS_LIST);
    assert!(response.starts_with("HTTP/1.1 404"), "{response}");
    assert!(seen.is_empty());
}

#[test]
fn lean_profile_still_refuses_list_documents() {
    let call = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_documents","arguments":{}}}"#;
    let (response, seen) = post("/mcp/lean", call);
    assert!(response.contains("tool-not-in-profile"), "{response}");
    assert!(
        seen.is_empty(),
        "a refused call never reaches the UI: {seen:?}"
    );
    // The six-tool lean list does not advertise it either, and the full list
    // does.
    let (lean, _) = post("/mcp/lean", TOOLS_LIST);
    assert!(!tool_names(&lean).contains(&"list_documents".to_string()));
    let (full, _) = post("/mcp", TOOLS_LIST);
    assert!(tool_names(&full).contains(&"list_documents".to_string()));
}

#[test]
fn the_lean_catalog_advertises_document_id_on_its_tools() {
    let (lean, _) = post("/mcp/lean", TOOLS_LIST);
    let body = lean.split_once("\r\n\r\n").expect("http body").1;
    let value: serde_json::Value = serde_json::from_str(body).expect("json");
    for tool in value["result"]["tools"].as_array().expect("tools") {
        assert!(
            tool["inputSchema"]["properties"]["documentId"].is_object(),
            "{tool}"
        );
    }
    // A route bound to one document needs no id and does not advertise it.
    let (bound, _) = post("/mcp/d/1/lean", TOOLS_LIST);
    assert!(!bound.contains("documentId"), "{bound}");
}
