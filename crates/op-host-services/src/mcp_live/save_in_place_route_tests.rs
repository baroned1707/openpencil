//! `save_document` without a `filePath` through the real `serve_connection`
//! router, with a stub UI thread feeding a real `McpLiveServer::pump`: the
//! connection-side routing, the operation log, the UI-side checks and the
//! atomic write are all production code.

use std::path::PathBuf;

use super::test_support::*;
use super::*;
use crate::disk_fingerprint::DiskFingerprint;

/// A scratch project with one saved design file.
struct Workspace {
    scratch: PathBuf,
}

impl Workspace {
    fn new(tag: &str) -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "openpencil-save-in-place-route-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join("proj/.git")).expect("git marker");
        std::fs::create_dir_all(scratch.join("proj/designs")).expect("designs");
        Self { scratch }
    }

    fn file(&self) -> PathBuf {
        self.scratch.join("proj/designs/a.op")
    }

    /// `d1` on screen, bound to `a.op` (which holds the starter document), with
    /// unsaved changes and the file's state recorded as its baseline.
    fn resolver(&self) -> TestResolver {
        let mut state = EditorState::starter();
        crate::doc_io::save_to_path(&state, &self.file()).expect("seed file");
        state.mark_saved_revision();
        state.mark_document_changed();
        let mut resolver = TestResolver::single(state);
        resolver.document_mut(1).path = Some(self.file());
        resolver.track_baselines();
        resolver
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

fn save_body(extra: &str) -> String {
    tool_call(7, "save_document", &format!("{}{extra}", document_arg(1)))
}

fn post_save(body: &str, resolver: TestResolver, documents: &LiveDocuments) -> Posted {
    post("/mcp", "", body, documents, resolver, None)
}

fn result_json(response: &str) -> serde_json::Value {
    serde_json::from_str(&crate::mcp_serve::tool_text(response)).expect("result JSON")
}

#[test]
fn save_document_without_file_path_saves_in_place_and_clears_dirty() {
    let workspace = Workspace::new("saves");
    let mut resolver = workspace.resolver();
    resolver.document_mut(1).state.add_page();
    let before = std::fs::read(workspace.file()).unwrap();

    let mut posted = post_save(&save_body(""), resolver, &LiveDocuments::default());

    assert!(!is_error(&posted.response), "{}", posted.response);
    let reply = result_json(&posted.response);
    assert_eq!(reply["saved"], serde_json::json!(true));
    assert_eq!(reply["path"], workspace.file().to_string_lossy().as_ref());
    assert_eq!(reply["documentId"], full(1).as_str());
    assert_eq!(reply["dirtyBefore"], serde_json::json!(true));
    let written = std::fs::read(workspace.file()).unwrap();
    assert_eq!(reply["bytes"], serde_json::json!(written.len()));
    assert_ne!(written, before, "the file now holds the agent's edit");
    let loaded =
        crate::doc_io::load_editor_state(&workspace.file(), op_editor_core::Locale::EnUs).unwrap();
    let state = &posted.ui.resolver.document_mut(1).state;
    assert_eq!(loaded.doc, state.doc);
    assert!(!state.is_dirty(), "the saved document is clean");
    assert_eq!(posted.ui.saved.len(), 1);
    assert_eq!(posted.ui.saved[0].1.path, workspace.file());
}

#[test]
fn save_in_place_refuses_untitled_with_actionable_error() {
    let mut state = EditorState::starter();
    state.mark_document_changed();

    let posted = post_save(
        &save_body(""),
        TestResolver::single(state),
        &LiveDocuments::default(),
    );

    assert!(is_error(&posted.response), "{}", posted.response);
    let text = error_text(&posted.response);
    assert!(text.contains("this document has no file yet"), "{text}");
    assert!(
        text.contains("create_document { projectRoot, name }"),
        "{text}"
    );
    assert!(text.contains("filePath"), "{text}");
    assert!(posted.ui.saved.is_empty());
}

#[test]
fn save_in_place_never_overwrites_a_file_changed_on_disk() {
    let workspace = Workspace::new("conflict");
    let resolver = workspace.resolver();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(workspace.file(), b"someone else's work").unwrap();

    let mut posted = post_save(&save_body(""), resolver, &LiveDocuments::default());

    assert!(is_error(&posted.response), "{}", posted.response);
    let text = error_text(&posted.response);
    assert!(
        text.contains("the file changed on disk since it was opened"),
        "{text}"
    );
    assert!(text.contains("open it again or use Save As"), "{text}");
    assert_eq!(
        std::fs::read(workspace.file()).unwrap(),
        b"someone else's work"
    );
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());
    assert!(posted.ui.saved.is_empty());
}

#[test]
fn a_save_still_running_refuses_the_in_place_save_without_writing() {
    let workspace = Workspace::new("inflight");
    let mut resolver = workspace.resolver();
    resolver.save_in_flight = true;
    let before = std::fs::read(workspace.file()).unwrap();

    let mut posted = post_save(&save_body(""), resolver, &LiveDocuments::default());

    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(error_text(&posted.response).contains("NOT saved"));
    assert_eq!(std::fs::read(workspace.file()).unwrap(), before);
    assert!(posted.ui.resolver.document_mut(1).state.is_dirty());
}

#[test]
fn retry_with_same_operation_id_saves_once() {
    let workspace = Workspace::new("retry");
    let documents = LiveDocuments::default();
    let body = save_body(r#","operationId":"save-1""#);

    let first = post_save(&body, workspace.resolver(), &documents);
    assert!(!is_error(&first.response), "{}", first.response);
    assert_eq!(first.ui.saved.len(), 1);
    let on_disk = std::fs::read(workspace.file()).unwrap();
    let fingerprint = DiskFingerprint::capture(&workspace.file()).unwrap();

    // The agent edits again, then retries the SAME operation (its first answer
    // was lost): the retry replays the first result and writes nothing.
    let mut resolver = first.ui.resolver;
    resolver.document_mut(1).state.mark_document_changed();
    let mut retried = post_save(&body, resolver, &documents);

    assert_eq!(
        crate::mcp_serve::tool_text(&retried.response),
        crate::mcp_serve::tool_text(&first.response),
        "the retry returns the first result"
    );
    assert_eq!(
        json_body(&retried.response)["result"]["_meta"]["replayed"],
        serde_json::json!(true)
    );
    assert!(retried.ui.saved.is_empty(), "no second write");
    assert_eq!(std::fs::read(workspace.file()).unwrap(), on_disk);
    assert_eq!(
        DiskFingerprint::capture(&workspace.file()).unwrap(),
        fingerprint
    );
    assert!(
        retried.ui.resolver.document_mut(1).state.is_dirty(),
        "the later edit is still unsaved: the retry did not save it"
    );
    // The same id with different arguments is a conflict, not a replay.
    let other = save_body(r#","operationId":"save-1","sourceFilePath":"/x/y.op""#);
    let conflicted = post_save(&other, retried.ui.resolver, &documents);
    assert!(is_error(&conflicted.response), "{}", conflicted.response);
}

#[test]
fn the_lean_route_does_not_offer_the_in_place_save() {
    let workspace = Workspace::new("lean");
    let posted = post(
        "/mcp/lean",
        "",
        &save_body(""),
        &LiveDocuments::default(),
        workspace.resolver(),
        None,
    );
    assert!(
        posted.response.contains("tool-not-in-profile"),
        "{}",
        posted.response
    );
    assert!(posted.ui.saved.is_empty());
}

#[test]
fn a_copy_save_with_a_source_file_is_not_the_in_place_form() {
    let call = op_mcp::parse_tool_call(&save_body(r#","sourceFilePath":"/a.op""#)).unwrap();
    assert!(!crate::save_in_place::is_in_place_save_call(&call));
    let blank = op_mcp::parse_tool_call(&save_body(r#","filePath":"  ""#)).unwrap();
    assert!(crate::save_in_place::is_in_place_save_call(&blank));
}

#[path = "save_own_path_route_tests.rs"]
mod own_path;
