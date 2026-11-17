//! `save_document` without a `filePath` end to end on a headless
//! `DesktopApp`: a real HTTP call to the live MCP server, answered by the real
//! `poll_mcp_server` pump, then the editor-side follow-ups (baseline, recovery
//! snapshot, tab row). Nested in `mcp_create_document_tests` for its fixtures.

use super::*;
use crate::recovery_host::test_support::{app_with_tabs, edit_active, files_in};
use op_host_services::disk_fingerprint::DiskFingerprint;

/// An app with `names` open as tabs (crash recovery on, in a temp store), the
/// live server running, and the last tab on screen.
fn app_with_file_and_server(tag: &str) -> (DesktopApp, u16, PathBuf, PathBuf, PathBuf) {
    let (mut app, paths, work, recovery) = app_with_tabs(tag, &["a.op"]);
    app.force_live_mcp_port = Some(0);
    assert!(app.reconcile_mcp_server_from_settings());
    let port = app.mcp_server.as_ref().expect("live MCP server").port();
    (app, port, paths[0].clone(), work, recovery)
}

fn save_without_file_path(app: &mut DesktopApp, port: u16) -> String {
    call_tool(app, port, "save_document", serde_json::json!({}))
}

fn reply(response: &str) -> serde_json::Value {
    serde_json::from_str(&op_host_services::mcp_serve::tool_text(response)).expect("result JSON")
}

fn saved_name(path: &Path) -> Option<String> {
    op_host_services::doc_io::load_editor_state(path, op_editor_core::Locale::EnUs)
        .unwrap()
        .doc
        .name
}

#[test]
fn save_document_without_file_path_saves_in_place_and_clears_dirty() {
    let (mut app, port, file, work, recovery) = app_with_file_and_server("mcp-save");
    edit_active(&mut app, "agent-edit");
    app.pump_recovery();
    app.flush_recovery();
    assert_eq!(files_in(&recovery).len(), 2, "snapshot + manifest exist");
    assert!(app.document_is_dirty());

    let response = save_without_file_path(&mut app, port);

    assert!(!response.contains(r#""isError":true"#), "{response}");
    let result = reply(&response);
    assert_eq!(result["saved"], serde_json::json!(true));
    assert_eq!(result["path"], file.to_string_lossy().as_ref());
    assert_eq!(result["dirtyBefore"], serde_json::json!(true));
    assert_eq!(
        result["bytes"],
        serde_json::json!(std::fs::metadata(&file).unwrap().len())
    );
    let document_id = result["documentId"].as_str().unwrap();
    let (number, nonce) = document_id[1..].split_once('.').expect("d<N>.<nonce>");
    assert!(document_id.starts_with('d') && number.parse::<u64>().is_ok());
    assert_eq!(nonce.len(), 6, "{document_id}");
    assert_eq!(saved_name(&file).as_deref(), Some("agent-edit"));
    assert!(!app.document_is_dirty());
    app.publish_document_tabs();
    assert!(!app.host.editor_state().editor_ui.document_tabs[0].dirty);
    assert!(files_in(&recovery).is_empty(), "{:?}", files_in(&recovery));
    assert_eq!(
        app.disk_baselines.get(&file),
        DiskFingerprint::capture(&file).ok()
    );
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

/// Invariant: a save the editor made itself (Cmd+S, or an earlier agent save)
/// is never mistaken for somebody else's change.
#[test]
fn user_saves_and_agent_saves_interleave_without_false_conflicts() {
    let (mut app, port, file, work, recovery) = app_with_file_and_server("mcp-interleave");
    edit_active(&mut app, "first");
    assert!(app.request_background_save() && app.finish_background_saves());

    edit_active(&mut app, "second");
    let response = save_without_file_path(&mut app, port);
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert_eq!(saved_name(&file).as_deref(), Some("second"));

    edit_active(&mut app, "third");
    let again = save_without_file_path(&mut app, port);
    assert!(!again.contains(r#""isError":true"#), "{again}");
    assert_eq!(saved_name(&file).as_deref(), Some("third"));

    edit_active(&mut app, "fourth");
    assert!(app.request_background_save() && app.finish_background_saves());
    assert_eq!(saved_name(&file).as_deref(), Some("fourth"));
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn a_file_changed_behind_the_editor_is_refused_end_to_end() {
    let (mut app, port, file, work, recovery) = app_with_file_and_server("mcp-conflict");
    // The first frame records the file as the editor opened it.
    app.poll_mcp_server();
    edit_active(&mut app, "agent-edit");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, b"someone else's work").unwrap();

    let response = save_without_file_path(&mut app, port);

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response.contains("the file changed on disk since it was opened"),
        "{response}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"someone else's work");
    assert!(app.document_is_dirty(), "the agent's work is still unsaved");
    let _ = std::fs::remove_dir_all(work);
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn an_untitled_document_gets_the_actionable_error_end_to_end() {
    let (mut app, port) = app_with_live_server();
    app.host.editor_state_mut().mark_document_changed();

    let response = save_without_file_path(&mut app, port);

    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response.contains("this document has no file yet"),
        "{response}"
    );
    assert!(
        response.contains("create_document { projectRoot, name }"),
        "{response}"
    );
    assert!(app.document_is_dirty());
}
