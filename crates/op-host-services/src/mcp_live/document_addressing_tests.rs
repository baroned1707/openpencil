//! Which document a live call reaches, how calls to different documents queue,
//! and idempotent retries — all through the real `serve_connection` router,
//! with a stub UI thread pumping a real `McpLiveServer`.

use super::test_support::*;
use super::*;

fn json_body(response: &str) -> serde_json::Value {
    let body = response.split_once("\r\n\r\n").expect("http body").1;
    serde_json::from_str(body).expect("json body")
}

fn is_error(response: &str) -> bool {
    json_body(response)["result"]["isError"] == serde_json::json!(true)
}

fn error_text(response: &str) -> String {
    json_body(response)["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn page_count(resolver: &mut TestResolver, document: u64) -> usize {
    op_mcp::list_pages_snapshot(&resolver.document_mut(document).state).page_count
}

fn call(path: &str, headers: &str, body: &str, resolver: TestResolver) -> Posted {
    post(
        path,
        headers,
        body,
        &LiveDocuments::default(),
        resolver,
        None,
    )
}

#[test]
fn single_open_document_is_auto_selected() {
    let posted = call(
        "/mcp",
        "",
        &tool_call(1, "get_document_info", ""),
        TestResolver::single(EditorState::new()),
    );

    assert!(!is_error(&posted.response), "{}", posted.response);
    assert_eq!(posted.ui.seen, ["documents", "snapshot"]);
}

#[test]
fn several_documents_without_id_is_refused_with_a_listing() {
    let posted = call(
        "/mcp",
        "",
        &tool_call(2, "get_document_info", ""),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );

    assert!(is_error(&posted.response), "{}", posted.response);
    let text = error_text(&posted.response);
    assert!(text.contains("d3 proj/doc3.op (active)"), "{text}");
    assert!(text.contains("d4 proj/doc4.op"), "{text}");
    assert!(text.contains("documentId"), "{text}");
    assert!(text.contains("list_documents"), "{text}");
    assert_eq!(
        posted.ui.seen,
        ["documents"],
        "nothing may be read or applied"
    );
}

#[test]
fn unknown_document_id_lists_the_open_ones() {
    for named in ["d9", "banana"] {
        let posted = call(
            "/mcp",
            "",
            &tool_call(
                3,
                "get_document_info",
                &format!(r#""documentId":"{named}""#),
            ),
            TestResolver::with_ids(&[(3, true), (4, false)]),
        );
        assert!(is_error(&posted.response), "{}", posted.response);
        let text = error_text(&posted.response);
        assert!(
            text.contains(&format!("unknown documentId '{named}'")),
            "{text}"
        );
        assert!(text.contains("d3 proj/doc3.op (active)"), "{text}");
        assert!(text.contains("d4 proj/doc4.op"), "{text}");
    }
}

#[test]
fn unknown_ids_do_not_leave_gates_behind() {
    let documents = LiveDocuments::default();
    let _ = post(
        "/mcp",
        "",
        &tool_call(3, "get_document_info", r#""documentId":"d9""#),
        &documents,
        TestResolver::with_ids(&[(3, true)]),
        None,
    );
    assert_eq!(documents.gate_count(), 0);
}

#[test]
fn route_bound_id_conflicting_with_argument_is_refused() {
    let posted = call(
        "/mcp/d/3",
        "",
        &tool_call(4, "get_document_info", r#""documentId":"d4""#),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );

    assert!(is_error(&posted.response), "{}", posted.response);
    let text = error_text(&posted.response);
    assert!(
        text.contains("conflicts with the document this endpoint is bound to (d3)"),
        "{text}"
    );
    assert!(posted.ui.seen.is_empty(), "refused before the UI thread");

    // The same id, or none, is fine: the route names the document.
    for arguments in [r#""documentId":"d3""#, ""] {
        let posted = call(
            "/mcp/d/3",
            "",
            &tool_call(5, "get_document_info", arguments),
            TestResolver::with_ids(&[(3, true), (4, false)]),
        );
        assert!(!is_error(&posted.response), "{}", posted.response);
        assert_eq!(
            posted.ui.seen,
            ["snapshot"],
            "a bound route needs no listing"
        );
    }
}

#[test]
fn document_id_argument_selects_that_document() {
    let mut resolver = TestResolver::with_ids(&[(3, true), (4, false)]);
    resolver.lend_parked = true;

    let mut posted = call(
        "/mcp",
        "",
        &tool_call(6, "add_page", r#""documentId":"d4","name":"Extra""#),
        resolver,
    );

    assert!(!is_error(&posted.response), "{}", posted.response);
    assert_eq!(page_count(&mut posted.ui.resolver, 4), 2, "d4 got the page");
    assert_eq!(page_count(&mut posted.ui.resolver, 3), 1, "d3 did not");
    assert_eq!(posted.ui.seen, ["snapshot", "apply"], "no listing needed");
}

#[test]
fn list_documents_reports_status_and_active() {
    let mut resolver = TestResolver::with_ids(&[(3, true), (4, false)]);
    resolver.document_mut(4).status = op_editor_core::DocumentTabStatus::Running;
    resolver.document_mut(4).state.mark_document_changed();

    let posted = call("/mcp", "", &tool_call(7, "list_documents", ""), resolver);

    assert!(!is_error(&posted.response), "{}", posted.response);
    let listed: serde_json::Value =
        serde_json::from_str(&crate::mcp_serve::tool_text(&posted.response)).expect("array");
    assert_eq!(listed[0]["documentId"], "d3");
    assert_eq!(listed[0]["active"], true);
    assert_eq!(listed[0]["status"], "idle");
    assert_eq!(listed[0]["path"], "/work/proj/doc3.op");
    assert_eq!(listed[0]["project"], "proj");
    assert_eq!(listed[1]["documentId"], "d4");
    assert_eq!(listed[1]["active"], false);
    assert_eq!(listed[1]["status"], "running");
    assert_eq!(listed[1]["dirty"], true);
    assert_eq!(
        posted.ui.seen,
        ["documents"],
        "no document is read or locked"
    );
}

#[test]
fn parked_document_is_reported_not_reachable() {
    let posted = call(
        "/mcp",
        "",
        &tool_call(8, "get_document_info", r#""documentId":"d4""#),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );

    assert!(is_error(&posted.response), "{}", posted.response);
    assert!(
        error_text(&posted.response)
            .contains("document d4 is open but not on screen yet; switch to it"),
        "{}",
        posted.response
    );
}

#[test]
fn a_catalog_request_with_several_documents_falls_back_to_the_active_one() {
    let posted = call(
        "/mcp",
        "",
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        TestResolver::with_ids(&[(3, false), (4, true)]),
    );
    let tools = json_body(&posted.response)["result"]["tools"].clone();
    assert!(tools.as_array().is_some_and(|tools| tools.len() > 100));
    // Every call-taking tool advertises the optional ids; none requires them.
    let info = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "get_document_info")
        .expect("get_document_info");
    assert_eq!(
        info["inputSchema"]["properties"]["documentId"]["x-mcp-header"],
        "DocumentId"
    );
    assert!(info["inputSchema"]["properties"]
        .get("operationId")
        .is_none());
    let write = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "add_page")
        .expect("add_page");
    assert!(write["inputSchema"]["properties"]["operationId"].is_object());
    assert!(write["inputSchema"]["required"]
        .as_array()
        .is_none_or(|required| !required
            .iter()
            .any(|name| name == "documentId" || name == "operationId")));
    let listed = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "list_documents")
        .expect("list_documents");
    assert!(listed["inputSchema"]["properties"]
        .get("documentId")
        .is_none());
}

#[test]
fn a_bound_endpoint_still_lists_tools_while_its_document_is_parked() {
    // The user switched tabs: d4 is no longer on screen, but the agent bound to
    // it must still be able to read the catalog (and is told why a call fails).
    let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    for path in ["/mcp/d/4", "/mcp/d/9"] {
        let posted = call(
            path,
            "",
            list,
            TestResolver::with_ids(&[(3, true), (4, false)]),
        );
        let tools = json_body(&posted.response)["result"]["tools"].clone();
        assert!(
            tools.as_array().is_some_and(|tools| tools.len() > 100),
            "{path}: {}",
            posted.response
        );
    }
    let blocked = call(
        "/mcp/d/4",
        "",
        &tool_call(2, "get_document_info", ""),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    assert!(error_text(&blocked.response).contains("not on screen yet"));
}

// ── per-document queues ─────────────────────────────────────────────────

/// A UI thread that answers by hand so a test controls the interleaving.
/// `on_snapshot` runs for each snapshot request before it is answered.
fn manual_ui(
    conn_rx: Receiver<UiRequest>,
    mut on_snapshot: impl FnMut(DocumentId, usize) + Send + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut open_calls = 0usize;
        while let Ok(request) = conn_rx.recv_timeout(Duration::from_secs(5)) {
            match request {
                UiRequest::Snapshot { document, ack, .. } => {
                    open_calls += 1;
                    on_snapshot(document, open_calls);
                    let state = EditorState::new();
                    let epoch = DocumentEpoch::of(0, &state);
                    let _ = ack.send(Ok(SnapshotAck { state, epoch }));
                }
                UiRequest::Apply { ack, .. } => {
                    open_calls -= 1;
                    let _ = ack.send(ApplyAck {
                        applied: true,
                        ..ApplyAck::default()
                    });
                }
                _ => {}
            }
        }
    })
}

fn concurrent_calls(first: &str, second: &str, ui_delay: Duration) -> usize {
    let documents = Arc::new(LiveDocuments::default());
    let (conn_tx, conn_rx) = mpsc::channel::<UiRequest>();
    let peak = Arc::new(AtomicUsize::new(0));
    let ui_peak = Arc::clone(&peak);
    let ui = manual_ui(conn_rx, move |_, open| {
        ui_peak.fetch_max(open, Ordering::AcqRel);
        thread::sleep(ui_delay);
    });
    let callers: Vec<_> = [first.to_string(), second.to_string()]
        .into_iter()
        .map(|argument| {
            let documents = Arc::clone(&documents);
            let conn_tx = conn_tx.clone();
            thread::spawn(move || {
                serve_one(
                    "/mcp",
                    "",
                    &tool_call(1, "clear_selection", &argument),
                    &documents,
                    &conn_tx,
                )
            })
        })
        .collect();
    for caller in callers {
        let response = caller.join().expect("caller");
        assert!(!is_error(&response), "{response}");
    }
    drop(conn_tx);
    ui.join().expect("ui");
    peak.load(Ordering::Acquire)
}

#[test]
fn calls_to_different_documents_run_concurrently() {
    // The UI sleeps inside the first snapshot, so the second call's snapshot
    // is queued behind it only if the two calls share a gate. Both being open
    // at once (peak 2) proves they do not.
    let peak = concurrent_calls(
        r#""documentId":"d3""#,
        r#""documentId":"d4""#,
        Duration::from_millis(300),
    );
    assert_eq!(peak, 2, "calls to different documents must overlap");
}

#[test]
fn calls_to_one_document_stay_serial() {
    let peak = concurrent_calls(
        r#""documentId":"d3""#,
        r#""documentId":"d3""#,
        Duration::from_millis(300),
    );
    assert_eq!(peak, 1, "calls to one document must queue");
}

// ── idempotent retries ──────────────────────────────────────────────────

fn add_page_call(id: i64, operation: &str, name: &str) -> String {
    tool_call(
        id,
        "add_page",
        &format!(r#""documentId":"d1","operationId":"{operation}","name":"{name}""#),
    )
}

#[test]
fn operation_id_replay_returns_stored_response_without_reapplying() {
    let documents = LiveDocuments::default();
    let mut first = post(
        "/mcp",
        "",
        &add_page_call(10, "op-1", "Landing"),
        &documents,
        TestResolver::single(EditorState::new()),
        None,
    );
    assert!(!is_error(&first.response), "{}", first.response);
    assert_eq!(page_count(&mut first.ui.resolver, 1), 2);
    assert_eq!(json_body(&first.response)["result"].get("_meta"), None);

    // The retry has its own request id and the same operation.
    let mut retry = post(
        "/mcp",
        "",
        &add_page_call(11, "op-1", "Landing"),
        &documents,
        first.ui.resolver,
        None,
    );

    assert!(
        retry.ui.seen.is_empty(),
        "a replay never reaches the UI thread"
    );
    assert_eq!(
        page_count(&mut retry.ui.resolver, 1),
        2,
        "not applied twice"
    );
    let replayed = json_body(&retry.response);
    assert_eq!(replayed["id"], 11);
    assert_eq!(replayed["result"]["_meta"]["replayed"], true);
    assert_eq!(
        replayed["result"]["content"],
        json_body(&first.response)["result"]["content"],
        "same stored answer"
    );
}

#[test]
fn operation_id_reuse_with_different_arguments_is_refused() {
    let documents = LiveDocuments::default();
    let first = post(
        "/mcp",
        "",
        &add_page_call(10, "op-1", "Landing"),
        &documents,
        TestResolver::single(EditorState::new()),
        None,
    );
    let mut second = post(
        "/mcp",
        "",
        &add_page_call(11, "op-1", "Pricing"),
        &documents,
        first.ui.resolver,
        None,
    );

    assert!(is_error(&second.response), "{}", second.response);
    assert!(
        error_text(&second.response)
            .contains("operationId 'op-1' was already used for a different call"),
        "{}",
        second.response
    );
    assert_eq!(page_count(&mut second.ui.resolver, 1), 2, "nothing applied");
}

#[test]
fn a_failed_write_is_not_remembered_and_reads_ignore_the_operation_id() {
    let documents = LiveDocuments::default();
    // A write refused by the document is retried for real, not replayed.
    let refused = post(
        "/mcp",
        "",
        &tool_call(
            1,
            "add_page",
            r#""documentId":"d1","operationId":"op-2","name":"  ""#,
        ),
        &documents,
        TestResolver::single(EditorState::new()),
        None,
    );
    assert!(is_error(&refused.response), "{}", refused.response);
    let retried = post(
        "/mcp",
        "",
        &add_page_call(2, "op-2", "Fine"),
        &documents,
        refused.ui.resolver,
        None,
    );
    // Different arguments under the same id would be a conflict if the first
    // had been stored; it was not, so this simply runs.
    assert!(!is_error(&retried.response), "{}", retried.response);

    // A read carrying an operationId runs every time.
    let read = tool_call(
        3,
        "get_document_info",
        r#""documentId":"d1","operationId":"op-3""#,
    );
    let first = post("/mcp", "", &read, &documents, retried.ui.resolver, None);
    let second = post("/mcp", "", &read, &documents, first.ui.resolver, None);
    assert_eq!(second.ui.seen, ["snapshot"]);
    assert_eq!(json_body(&second.response)["result"].get("_meta"), None);
}

// ── headless / single document ──────────────────────────────────────────

#[test]
fn headless_mode_single_document_works_without_id() {
    // The pump over a host with exactly one document, `d1`.
    let (conn_tx, conn_rx) = mpsc::channel::<UiRequest>();
    let ui = thread::spawn(move || {
        let (mut server, server_tx) = server_with_queue();
        let mut state = EditorState::new();
        while let Ok(request) = conn_rx.recv_timeout(Duration::from_secs(5)) {
            server_tx.send(request).expect("forward");
            server.pump(&mut SingleDocumentResolver::new(&mut state));
        }
        state.active_children().len()
    });
    let documents = LiveDocuments::default();

    let plain = serve_one(
        "/mcp",
        "",
        &tool_call(1, "get_document_info", ""),
        &documents,
        &conn_tx,
    );
    let named = serve_one(
        "/mcp",
        "",
        &tool_call(2, "get_document_info", r#""documentId":"d1""#),
        &documents,
        &conn_tx,
    );
    let other = serve_one(
        "/mcp",
        "",
        &tool_call(3, "get_document_info", r#""documentId":"d2""#),
        &documents,
        &conn_tx,
    );
    drop(conn_tx);
    ui.join().expect("ui");

    assert!(!is_error(&plain), "{plain}");
    assert!(!is_error(&named), "{named}");
    assert!(is_error(&other), "{other}");
    assert!(error_text(&other).contains("d1"), "{other}");
}

// ── Mcp-Param-DocumentId ────────────────────────────────────────────────

#[test]
fn header_mismatch_is_refused_when_header_present() {
    let body = tool_call(9, "get_document_info", r#""documentId":"d3""#);
    let resolver = || TestResolver::with_ids(&[(3, true), (4, false)]);

    let mismatch = call("/mcp", "Mcp-Param-DocumentId: d4\r\n", &body, resolver());
    assert!(
        mismatch.response.starts_with("HTTP/1.1 400"),
        "{}",
        mismatch.response
    );
    let error = json_body(&mismatch.response);
    assert_eq!(error["error"]["code"], -32020);
    assert_eq!(error["id"], 9);
    assert!(mismatch.ui.seen.is_empty(), "refused before the UI thread");

    // Matching (plain and base64-encoded), and absent headers are served.
    for header in [
        "Mcp-Param-DocumentId: d3\r\n",
        "mcp-param-documentid: =?base64?ZDM=?=\r\n",
        "",
    ] {
        let posted = call("/mcp", header, &body, resolver());
        assert!(
            posted.response.starts_with("HTTP/1.1 200"),
            "{header}: {}",
            posted.response
        );
        assert!(!is_error(&posted.response), "{header}: {}", posted.response);
    }
    // A header with no body value to match is a mismatch too.
    let no_body_value = call(
        "/mcp",
        "Mcp-Param-DocumentId: d3\r\n",
        &tool_call(9, "get_document_info", ""),
        resolver(),
    );
    assert!(
        no_body_value.response.starts_with("HTTP/1.1 400"),
        "{}",
        no_body_value.response
    );
}
