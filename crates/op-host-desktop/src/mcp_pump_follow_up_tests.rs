//! The desktop's follow-ups to one MCP pump land on the document each request
//! was about. A pump answers up to eight requests, possibly about different
//! tabs, so "the document the last request addressed" is not the document a
//! replace or a created file belongs to.
//!
//! Real HTTP against the real live server, answered by `poll_mcp_server`; the
//! test controls WHEN the pump runs so that two requests are queued together.

use super::multi_document_e2e_support::*;
use crate::message_dialog::Choice;
use serde_json::json;
use std::time::Duration;

/// Time for a client thread's connection to enqueue its next UI request.
const SETTLE: Duration = Duration::from_millis(250);

fn replace_body() -> String {
    let document =
        serde_json::to_string(&op_editor_core::EditorState::starter().doc).expect("document json");
    format!(r#"{{"document":{document}}}"#)
}

/// Replace the document on screen over REST and, in the SAME pump, read the
/// parked document `reader` over MCP. Returns once both clients are answered.
fn replace_on_screen_and_read_parked_in_one_pump(
    fx: &mut Fixture,
    reader: op_editor_core::DocumentId,
) {
    let port = fx.port;
    let body = replace_body();
    let replacer = std::thread::spawn(move || post_raw(port, "/api/mcp/document", &body));
    // The REST route first asks which document is on screen; answer that, then
    // leave its ReplaceDocument request queued.
    std::thread::sleep(SETTLE);
    fx.app.poll_mcp_server();
    std::thread::sleep(SETTLE);
    // A call that names its document with this launch's nonce needs no
    // listing round trip, so its request is queued behind the replace.
    let route = Fixture::route(reader);
    let read = std::thread::spawn(move || post_tool(port, &route, "list_pages", json!({})));
    std::thread::sleep(SETTLE);
    // One pump now holds [ReplaceDocument(on screen), read(parked)].
    fx.app.poll_mcp_server();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !(replacer.is_finished() && read.is_finished()) {
        fx.app.poll_mcp_server();
        assert!(std::time::Instant::now() < deadline, "clients not answered");
        std::thread::sleep(Duration::from_millis(5));
    }
    let replaced = replacer.join().expect("replace client").expect("HTTP");
    assert_eq!(replaced.status, 200, "{:?}", replaced.body);
    let read = read.join().expect("read client").expect("HTTP");
    assert!(!read.is_error(), "{}", read.text());
}

#[test]
fn replace_on_a_parked_tab_marks_only_that_tab_dirty_even_when_other_requests_share_the_pump() {
    let mut fx = Fixture::new("pump-replace");
    assert!(!fx.is_dirty(fx.a) && !fx.is_dirty(fx.c));

    let reader = fx.a;
    replace_on_screen_and_read_parked_in_one_pump(&mut fx, reader);

    assert!(
        fx.is_dirty(fx.c),
        "the replaced (on-screen) tab must be marked dirty"
    );
    assert!(
        !fx.is_dirty(fx.a),
        "the parked tab that was only read must stay clean"
    );
}

#[test]
fn mcp_replaced_tab_is_dirty_and_close_prompts() {
    let mut fx = Fixture::new("pump-close");

    let reader = fx.b;
    replace_on_screen_and_read_parked_in_one_pump(&mut fx, reader);

    let index = fx.app.documents.index_of(fx.c).expect("tab c is open");
    let mut asked = 0;
    let closed = fx.app.close_document_tab_with(index, |_, _| {
        asked += 1;
        Some(Choice::Cancel)
    });
    assert_eq!(asked, 1, "closing a tab replaced over MCP must ask first");
    assert!(!closed, "Cancel keeps the replaced tab");
}

#[test]
fn created_file_binds_the_requesting_documents_tab_when_two_requests_share_a_pump() {
    let mut fx = Fixture::new("pump-create");
    // An untitled tab that is parked while `c` is on screen.
    assert!(fx.app.activate_document_slot(0));
    assert!(fx.app.open_untitled_tab());
    let untitled = fx.app.documents.active_id();
    fx.app.host.editor_state_mut().mark_document_changed();
    assert!(fx.app.activate_document_slot(2));
    assert_eq!(fx.app.documents.active_id(), fx.c);
    let project = fx.p1.clone();

    let reply = fx.call(
        &Fixture::route(untitled),
        "create_document",
        json!({"projectRoot": project.to_string_lossy(), "name": "fresh"}),
    );

    assert!(!reply.is_error(), "{}", reply.text());
    let expected = project.join("designs").join("fresh.op");
    assert!(expected.is_file());
    fx.app.sync_active_slot();
    let paths = fx.app.documents.slot_paths();
    let untitled_index = fx.app.documents.index_of(untitled).expect("untitled tab");
    assert_eq!(paths[untitled_index].as_deref(), Some(expected.as_path()));
    assert_eq!(
        fx.app.current_path.as_deref(),
        Some(fx.path_c.as_path()),
        "the on-screen tab keeps its own file"
    );
    assert!(!fx.is_dirty(untitled), "created file is saved");
}

#[test]
fn an_outcome_naming_a_parked_tab_changes_that_tab_and_no_other() {
    let mut fx = Fixture::new("pump-outcome");
    let created = fx.p1.join("designs").join("made.op");
    let outcome = op_host_services::mcp_live::McpPumpOutcome {
        replaced: vec![fx.a],
        created: vec![(fx.b, created.clone())],
        ..Default::default()
    };

    fx.app.apply_mcp_pump_outcome(&outcome);

    assert!(fx.is_dirty(fx.a), "the replaced parked tab is dirty");
    assert!(!fx.is_dirty(fx.b) && !fx.is_dirty(fx.c));
    fx.app.sync_active_slot();
    let paths = fx.app.documents.slot_paths();
    let index_b = fx.app.documents.index_of(fx.b).expect("tab b");
    assert_eq!(paths[index_b].as_deref(), Some(created.as_path()));
    assert_eq!(
        fx.app.current_path.as_deref(),
        Some(fx.path_c.as_path()),
        "the on-screen tab keeps its file"
    );
}
