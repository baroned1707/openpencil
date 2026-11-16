//! `create_document` through the real `serve_connection` router with a stub
//! UI thread feeding a real `McpLiveServer::pump`, so the connection-side
//! validation, the epoch check and the UI-side write are all the production
//! code. Directories live under the OS temp dir; the home directory is the
//! machine's real one but is only ever compared against, never written.

use super::test_support::*;
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

/// POST `body` to the router over `resolver`. `after_snapshot` runs on the UI
/// thread between the call's snapshot and its create request.
fn call(
    body: &str,
    resolver: TestResolver,
    after_snapshot: Option<AfterSnapshot>,
) -> (String, UiRun) {
    call_at("/mcp", body, resolver, after_snapshot)
}

fn call_at(
    route: &str,
    body: &str,
    resolver: TestResolver,
    after_snapshot: Option<AfterSnapshot>,
) -> (String, UiRun) {
    let posted = post(
        route,
        "",
        body,
        &LiveDocuments::default(),
        resolver,
        after_snapshot,
    );
    (posted.response, posted.ui)
}

/// An untitled document with unsaved content, as the editor starts.
fn untitled() -> TestResolver {
    let mut state = EditorState::starter();
    state.mark_document_changed();
    TestResolver::single(state)
}

fn never_reaches_the_ui() -> Option<AfterSnapshot> {
    Some(Box::new(|_| {
        panic!("a refused call must not reach the UI thread");
    }))
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

    let (response, mut ui) = call(&create_call(&tree.0, "landing"), untitled(), None);

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
    let state = &ui.resolver.document_mut(1).state;
    assert_eq!(written.doc, state.doc);
    // ...the pump asked the host to bind it, and marked the document clean
    // in the same step as the write.
    assert_eq!(ui.created, vec![expected.clone()]);
    assert!(!state.is_dirty(), "the saved document is clean");
    assert_eq!(
        state.editor_ui.file_name_display.as_deref(),
        Some("landing.op")
    );
    // Once the host has bound the file, a follow-up call naming it is refused.
    let follow_up = format!(
        r#"{{"jsonrpc":"2.0","id":22,"method":"tools/call","params":{{"name":"add_page","arguments":{{"filePath":{},"name":"X"}}}}}}"#,
        serde_json::to_string(&expected.to_string_lossy()).unwrap()
    );
    let (refused, _) = call(&follow_up, ui.resolver, never_reaches_the_ui());
    assert!(refused.contains("omit filePath"), "{refused}");
}

#[test]
fn create_document_refuses_when_document_already_has_a_file() {
    let tree = Tree::new();
    let saved = tree.0.join("already.op");
    let mut resolver = untitled();
    resolver.document_mut(1).path = Some(saved.clone());

    let (response, ui) = call(
        &create_call(&tree.0, "second"),
        resolver,
        never_reaches_the_ui(),
    );

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
    let saved = tree.0.join("bound-meanwhile.op");

    let (response, ui) = call(
        &create_call(&tree.0, "late"),
        untitled(),
        Some(Box::new(move |resolver| {
            resolver.document_mut(1).path = Some(saved);
        })),
    );

    assert!(response.contains("already saved at"), "{response}");
    assert!(!tree.0.join("designs").exists());
    assert!(ui.created.is_empty());
}

#[test]
fn create_document_is_refused_after_the_document_changed_mid_call() {
    let tree = Tree::new();

    let (response, mut ui) = call(
        &create_call(&tree.0, "stale"),
        untitled(),
        Some(Box::new(|resolver| {
            resolver
                .document_mut(1)
                .state
                .replace_document(EditorState::new().doc);
        })),
    );

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response
            .contains("the open document changed while this call was running; re-read and retry"),
        "{response}"
    );
    assert!(!tree.0.join("designs").exists(), "nothing may be written");
    assert!(ui.created.is_empty());
    assert_eq!(ui.resolver.document_mut(1).path, None);
}

#[test]
fn bad_arguments_are_refused_without_waking_the_ui_thread() {
    let tree = Tree::new();
    for (body, expected) in [
        (create_call(&tree.0, "../escape"), "name is not a usable file name"),
        (create_call(Path::new("relative/dir"), "x"), "must be an absolute path"),
        (
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"create_document","arguments":{"name":"x"}}}"#.to_string(),
            "projectRoot is required",
        ),
    ] {
        let (response, ui) = call(&body, untitled(), never_reaches_the_ui());
        assert!(response.contains(r#""isError":true"#), "{response}");
        assert!(response.contains(expected), "{response}");
        assert!(ui.created.is_empty());
    }
}

#[test]
fn the_lean_route_still_refuses_create_document_as_not_in_profile() {
    let tree = Tree::new();

    let (response, ui) = call_at("/mcp/lean", &create_call(&tree.0, "x"), untitled(), None);

    assert!(response.contains("tool-not-in-profile"), "{response}");
    assert!(!tree.0.join("designs").exists());
    assert!(ui.created.is_empty());
}

#[test]
fn create_document_binds_the_addressed_untitled_document() {
    // With several documents open, the call names the one it means.
    let tree = Tree::new();
    let mut resolver = untitled();
    resolver.documents[0].id = DocumentId(3);
    let body = create_call(&tree.0, "named").replacen(
        r#""arguments":{"#,
        &format!(r#""arguments":{{{},"#, document_arg(3)),
        1,
    );

    let (response, ui) = call(&body, resolver, None);

    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert_eq!(ui.created, vec![tree.0.join("designs").join("named.op")]);
}
