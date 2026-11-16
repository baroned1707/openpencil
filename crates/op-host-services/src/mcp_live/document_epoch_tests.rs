//! A live tool call is a snapshot followed by an apply. The apply must be
//! refused when the addressed document was replaced in between (node ids
//! repeat across documents), and must still land after ordinary edits. The
//! epoch is per document: another document's replacement is none of its
//! business.

use super::test_support::*;
use super::*;

fn take_snapshot(
    server: &mut McpLiveServer,
    req_tx: &Sender<UiRequest>,
    resolver: &mut TestResolver,
    document: u64,
) -> DocumentEpoch {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Snapshot {
            document: DocumentId(document),
            ack: ack_tx,
        })
        .expect("queue snapshot");
    server.pump(resolver);
    match ack_rx.try_recv().expect("snapshot ack") {
        Ok(ack) => ack.epoch,
        Err(error) => panic!("snapshot refused: {error:?}"),
    }
}

fn apply_insert(
    server: &mut McpLiveServer,
    req_tx: &Sender<UiRequest>,
    resolver: &mut TestResolver,
    document: u64,
    expected_epoch: Option<DocumentEpoch>,
) -> ApplyAck {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Apply {
            document: DocumentId(document),
            tool_name: "insert_node".to_string(),
            cmd: EditorCommand::InsertNode {
                kind: "rect".to_string(),
                name: "Box".to_string(),
                x: 0,
                y: 0,
                width: 10,
                height: 10,
                fill_hex: None,
                target_parent: op_editor_core::NodeId::NONE,
                page_id: None,
            },
            expected_epoch,
            ack: ack_tx,
        })
        .expect("queue apply");
    server.pump(resolver);
    ack_rx.try_recv().expect("apply ack")
}

fn node_count(state: &EditorState) -> usize {
    state.active_children().len()
}

#[test]
fn apply_rejected_after_document_epoch_change() {
    let (mut server, req_tx) = server_with_queue();
    let mut resolver = TestResolver::single(EditorState::new());
    let epoch = take_snapshot(&mut server, &req_tx, &mut resolver, 1);

    // The document is replaced between the two halves of the call (whole-
    // document sync through the pump, or Open/New through the host).
    resolver
        .document_mut(1)
        .state
        .replace_document(EditorState::new().doc);
    let nodes_before = node_count(&resolver.document_mut(1).state);
    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 1, Some(epoch));
    assert!(!ack.applied && ack.document_changed);
    assert_eq!(
        node_count(&resolver.document_mut(1).state),
        nodes_before,
        "nothing may be applied"
    );

    // Open/New swaps the whole `EditorState` (generation restarts at 0), so
    // the host's own epoch has to be part of the identity too.
    let mut fresh = TestResolver::single(EditorState::new());
    let epoch = take_snapshot(&mut server, &req_tx, &mut fresh, 1);
    fresh.document_mut(1).host_epoch = 1;
    let ack = apply_insert(&mut server, &req_tx, &mut fresh, 1, Some(epoch));
    assert!(!ack.applied && ack.document_changed);
    assert_eq!(node_count(&fresh.document_mut(1).state), 0);
}

#[test]
fn epoch_is_checked_against_the_addressed_document() {
    let (mut server, req_tx) = server_with_queue();
    // Two reachable documents (the desktop's second one is parked, but the
    // epoch rule is the same for any document the resolver can lend).
    let mut resolver = TestResolver::with_ids(&[(1, true), (2, true)]);
    let first = take_snapshot(&mut server, &req_tx, &mut resolver, 1);
    let second = take_snapshot(&mut server, &req_tx, &mut resolver, 2);

    // Document 2 is replaced; document 1 is untouched.
    resolver
        .document_mut(2)
        .state
        .replace_document(EditorState::new().doc);

    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 2, Some(second));
    assert!(ack.document_changed && !ack.applied, "d2's epoch moved");
    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 1, Some(first));
    assert!(ack.applied && !ack.document_changed, "d1's epoch did not");
    assert_eq!(node_count(&resolver.document_mut(1).state), 1);
    assert_eq!(node_count(&resolver.document_mut(2).state), 0);

    // An epoch taken from d1 never validates an apply to d2.
    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 2, Some(first));
    assert!(ack.document_changed && !ack.applied);
}

#[test]
fn apply_accepted_when_epoch_unchanged_by_ordinary_edit() {
    let (mut server, req_tx) = server_with_queue();
    let mut resolver = TestResolver::single(EditorState::new());
    let epoch = take_snapshot(&mut server, &req_tx, &mut resolver, 1);

    // Ordinary edits (a user drag, an earlier tool call) change the revision
    // but not the document's identity.
    let first = apply_insert(&mut server, &req_tx, &mut resolver, 1, None);
    assert!(first.applied);
    let revision = resolver.document_mut(1).state.document_revision();
    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 1, Some(epoch));

    assert!(ack.applied && !ack.document_changed);
    let state = &resolver.document_mut(1).state;
    assert_ne!(state.document_revision(), revision, "the edit landed");
    assert_eq!(node_count(state), 2);
}

#[test]
fn apply_without_a_snapshot_is_never_epoch_checked() {
    let (mut server, req_tx) = server_with_queue();
    let mut resolver = TestResolver::single(EditorState::new());
    resolver.document_mut(1).host_epoch = 7;
    resolver
        .document_mut(1)
        .state
        .replace_document(EditorState::new().doc);

    let ack = apply_insert(&mut server, &req_tx, &mut resolver, 1, None);

    assert!(ack.applied);
}

const CLEAR_SELECTION: &str = r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"clear_selection","arguments":{}}}"#;

#[test]
fn stale_apply_reaches_the_client_as_a_tool_error_naming_the_cause() {
    let response = post(
        "/mcp",
        "",
        CLEAR_SELECTION,
        &LiveDocuments::default(),
        TestResolver::single(EditorState::new()),
        Some(Box::new(|resolver| {
            resolver
                .document_mut(1)
                .state
                .replace_document(EditorState::new().doc);
        })),
    )
    .response;

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(
        response
            .contains("the open document changed while this call was running; re-read and retry"),
        "{response}"
    );
    assert!(response.contains(r#""id":9"#), "{response}");
    assert!(!response.contains("host rejected command"), "{response}");
}

#[test]
fn unchanged_document_apply_succeeds_through_the_router() {
    let response = post(
        "/mcp",
        "",
        CLEAR_SELECTION,
        &LiveDocuments::default(),
        TestResolver::single(EditorState::new()),
        None,
    )
    .response;

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(!response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains(r#"\"wrote\":\"true\""#), "{response}");
}

#[test]
fn router_refuses_file_path_naming_the_open_document_without_asking_the_ui() {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-router-open-document-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let open = dir.join("open.op");
    let original = r#"{"version":"1.0.0","children":[]}"#;
    std::fs::write(&open, original).expect("write doc");
    let mut resolver = TestResolver::single(EditorState::new());
    resolver.document_mut(1).path = Some(open.clone());
    let target = serde_json::to_string(&open.to_string_lossy()).expect("path json");
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"add_page","arguments":{{"filePath":{target},"name":"FromAgent"}}}}}}"#
    );

    let posted = post(
        "/mcp",
        "",
        &body,
        &LiveDocuments::default(),
        resolver,
        Some(Box::new(|_| {
            panic!("a refused filePath must not reach the UI thread");
        })),
    );

    let response = posted.response;
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains(r#""isError":true"#), "{response}");
    assert!(response.contains("omit filePath"), "{response}");
    assert_eq!(std::fs::read_to_string(&open).expect("read"), original);
    let _ = std::fs::remove_dir_all(dir);
}
