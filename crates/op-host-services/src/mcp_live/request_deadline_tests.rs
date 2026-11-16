//! Request deadlines: a UI-thread request whose caller gave up must never be
//! applied later, and the agent must be told unambiguously which of "not
//! applied" and "may have been applied" happened.
//!
//! The tests drive the real `serve_connection` router against a real
//! `McpLiveServer::pump`, with the test thread standing in for the UI thread:
//! it takes requests off the queue itself, so it decides when (or whether) the
//! pump sees each one, and it moves a shared [`TestClock`] instead of sleeping
//! through the 15 s budget.

use super::live_timing::{channel, STARTED_GRACE};
use super::request_ticket::TicketState;
use std::path::PathBuf;

use super::test_support::*;
use super::*;

const BUDGET_OVER: Duration = Duration::from_millis(5_001);
const HOST_BUSY: &str =
    "the editor did not respond in time (a dialog may be open); the change was NOT applied — retry";
const OUTCOME_UNKNOWN: &str =
    "the change may or may not have been applied; read the document before retrying";

fn json_body(response: &str) -> serde_json::Value {
    let body = response.split_once("\r\n\r\n").expect("http body").1;
    serde_json::from_str(body).expect("json body")
}

fn is_error(response: &str) -> bool {
    json_body(response)["result"]["isError"] == serde_json::json!(true)
}

/// The tool error's message, without the envelope's `Error: ` prefix.
fn error_text(response: &str) -> String {
    let text = json_body(response)["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
}

fn add_page_call(id: i64, document: u64, operation: &str, name: &str) -> String {
    tool_call(
        id,
        "add_page",
        &format!(
            r#""documentId":"{}","operationId":"{operation}","name":"{name}""#,
            full(document)
        ),
    )
}

fn document_of(request: &UiRequest) -> Option<DocumentId> {
    match request {
        UiRequest::Snapshot { document, .. } | UiRequest::Apply { document, .. } => Some(*document),
        _ => None,
    }
}

/// The connection side, the pump and a host, with the test as the UI thread.
struct Rig {
    clock: TestClock,
    documents: Arc<LiveDocuments>,
    link: UiSender,
    /// What the connection threads send; the test decides what the pump sees.
    queue: Receiver<UiRequest>,
    server: McpLiveServer,
    server_tx: Sender<UiRequest>,
    resolver: TestResolver,
    created: Vec<PathBuf>,
}

impl Rig {
    fn new(resolver: TestResolver) -> Self {
        let clock = TestClock::new();
        let (link, queue) = channel(clock.timing());
        let (server, server_tx) = server_with_timing(clock.timing());
        Self {
            clock,
            documents: Arc::new(LiveDocuments::default()),
            link,
            queue,
            server,
            server_tx,
            resolver,
            created: Vec::new(),
        }
    }

    fn single() -> Self {
        Self::new(TestResolver::single(EditorState::new()))
    }

    /// POST `body` to the router on its own thread.
    fn call(&self, body: &str) -> thread::JoinHandle<String> {
        let documents = Arc::clone(&self.documents);
        let link = self.link.clone();
        let body = body.to_string();
        thread::spawn(move || serve_one_via("/mcp", "", &body, &documents, &link))
    }

    fn next_request(&self) -> UiRequest {
        self.queue
            .recv_timeout(Duration::from_secs(5))
            .expect("the connection should have queued a request")
    }

    /// Hand a request to the pump, as the UI thread does when it gets to it.
    fn deliver(&mut self, request: UiRequest) {
        self.server_tx.send(request).expect("queue for the pump");
        let outcome = self.server.pump(&mut self.resolver);
        for (document, path) in outcome.created {
            self.created.push(path.clone());
            self.resolver.bind_created(document, path);
        }
    }

    /// Serve requests normally until one of `kind` arrives; return it
    /// UNserved, as if the UI thread were stuck right before it.
    fn serve_until(&mut self, kind: &str) -> UiRequest {
        loop {
            let request = self.next_request();
            if request.kind() == kind {
                return request;
            }
            self.deliver(request);
        }
    }

    /// Serve everything until the call returns.
    fn serve_to_the_end(&mut self, call: thread::JoinHandle<String>) -> String {
        let patience = std::time::Instant::now() + Duration::from_secs(10);
        while !call.is_finished() {
            assert!(std::time::Instant::now() < patience, "the call hung");
            if let Ok(request) = self.queue.recv_timeout(Duration::from_millis(10)) {
                self.deliver(request);
            }
        }
        call.join().expect("call thread")
    }

    /// Move the clock `step` at a time until the (stuck) call returns.
    fn expire(&self, call: thread::JoinHandle<String>, step: Duration) -> String {
        let patience = std::time::Instant::now() + Duration::from_secs(10);
        while !call.is_finished() {
            assert!(std::time::Instant::now() < patience, "the call hung");
            self.clock.advance(step);
            thread::sleep(Duration::from_millis(10));
        }
        call.join().expect("call thread")
    }

    fn page_count(&mut self, document: u64) -> usize {
        op_mcp::list_pages_snapshot(&self.resolver.document_mut(document).state).page_count
    }

    fn log_len(&self, document: u64) -> usize {
        lock_gate(&self.documents.gate(DocumentId(document))).len()
    }
}

fn wait_for_grace(rig: &Rig) {
    let patience = std::time::Instant::now() + Duration::from_secs(5);
    while rig.link.timing().grace_waits() == 0 {
        assert!(
            std::time::Instant::now() < patience,
            "the caller never entered its grace wait"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn apply_request(
    clock: &TestClock,
    deadline_in: Duration,
) -> (UiRequest, Arc<Ticket>, Receiver<ApplyAck>) {
    let ticket = Ticket::new(clock.now() + deadline_in);
    let (ack, ack_rx) = mpsc::sync_channel(1);
    let request = UiRequest::Apply {
        document: DocumentId(1),
        tool_name: "add_page".to_string(),
        cmd: EditorCommand::AddPage {
            name: Some("Late".to_string()),
            children: None,
        },
        expected_epoch: None,
        ticket: Arc::clone(&ticket),
        ack,
    };
    (request, ticket, ack_rx)
}

// ── the pump ────────────────────────────────────────────────────────────

#[test]
fn expired_apply_is_dropped_by_the_pump_and_never_applied() {
    let clock = TestClock::new();
    let (mut server, queue) = server_with_timing(clock.timing());
    let mut resolver = TestResolver::single(EditorState::new());
    // Nobody cancelled it: the pump alone has to notice the deadline.
    let (request, ticket, ack_rx) = apply_request(&clock, Duration::from_secs(5));
    queue.send(request).expect("queue apply");
    clock.advance(BUDGET_OVER);

    let outcome = server.pump(&mut resolver);

    assert!(!outcome.repaint, "nothing ran");
    assert_eq!(
        op_mcp::list_pages_snapshot(&resolver.document_mut(1).state).page_count,
        1,
        "the expired write must not land"
    );
    assert_eq!(server.expired_requests_dropped(), 1);
    assert_eq!(ticket.state(), TicketState::Cancelled);
    assert!(
        matches!(ack_rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)),
        "the (already gone) caller's channel is closed, not answered with a result"
    );
}

#[test]
fn a_request_started_before_its_deadline_completes_normally() {
    let clock = TestClock::new();
    let (mut server, queue) = server_with_timing(clock.timing());
    let mut resolver = TestResolver::single(EditorState::new());
    let (request, ticket, ack_rx) = apply_request(&clock, Duration::from_secs(5));
    queue.send(request).expect("queue apply");
    clock.advance(Duration::from_secs(4));

    let outcome = server.pump(&mut resolver);

    assert!(outcome.repaint);
    assert!(ack_rx.try_recv().expect("answered").applied);
    assert_eq!(ticket.state(), TicketState::Done);
    assert_eq!(server.expired_requests_dropped(), 0);
}

#[test]
fn a_cancelled_request_is_dropped_even_before_its_deadline() {
    let clock = TestClock::new();
    let (mut server, queue) = server_with_timing(clock.timing());
    let mut resolver = TestResolver::single(EditorState::new());
    let (request, ticket, _ack_rx) = apply_request(&clock, Duration::from_secs(5));
    queue.send(request).expect("queue apply");
    // The caller gave up (for instance its thread died) well inside the budget.
    assert_eq!(ticket.cancel(), request_ticket::Cancel::Won);

    server.pump(&mut resolver);

    assert_eq!(
        op_mcp::list_pages_snapshot(&resolver.document_mut(1).state).page_count,
        1
    );
    assert_eq!(server.expired_requests_dropped(), 1);
}

#[test]
fn stale_requests_do_not_use_up_the_pump_budget_of_live_ones() {
    let clock = TestClock::new();
    let (mut server, queue) = server_with_timing(clock.timing());
    let mut resolver = TestResolver::single(EditorState::new());
    let mut live = Vec::new();
    for _ in 0..UI_PUMP_REQUEST_BUDGET {
        let (stale, _, _) = apply_request(&clock, Duration::ZERO);
        queue.send(stale).expect("queue stale");
        let (fresh, _, ack_rx) = apply_request(&clock, Duration::from_secs(5));
        queue.send(fresh).expect("queue live");
        live.push(ack_rx);
    }

    server.pump(&mut resolver);

    let answered = live.iter().filter(|ack| ack.try_recv().is_ok()).count();
    assert_eq!(answered, UI_PUMP_REQUEST_BUDGET);
    assert_eq!(
        server.expired_requests_dropped(),
        UI_PUMP_REQUEST_BUDGET as u64
    );
}

// ── the caller's verdict ────────────────────────────────────────────────

#[test]
fn caller_reports_host_busy_only_when_the_request_was_cancelled_before_start() {
    let mut rig = Rig::single();
    let call = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let stale = rig.serve_until("apply");
    let ticket = stale.ticket().cloned().expect("apply has a ticket");

    // The UI thread stays stuck past the caller's budget.
    rig.clock.advance(BUDGET_OVER);
    let response = call.join().expect("call");

    assert!(is_error(&response), "{response}");
    assert_eq!(error_text(&response), HOST_BUSY);
    assert_eq!(
        ticket.state(),
        TicketState::Cancelled,
        "the caller won the race"
    );
    // The UI thread comes back and finds the request already dead.
    rig.deliver(stale);
    assert_eq!(
        rig.page_count(1),
        1,
        "the write the agent was told failed never lands"
    );
    assert_eq!(rig.server.expired_requests_dropped(), 1);
}

#[test]
fn started_request_gets_a_grace_period_and_reports_its_real_ack() {
    let mut rig = Rig::single();
    let call = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let UiRequest::Apply {
        cmd, ticket, ack, ..
    } = rig.serve_until("apply")
    else {
        panic!("apply request");
    };
    // The UI thread has begun applying when the caller's budget runs out.
    assert!(ticket.try_start(rig.clock.now()));
    rig.clock.advance(BUDGET_OVER);
    wait_for_grace(&rig);
    assert!(
        !call.is_finished(),
        "a started request is not reported as failed"
    );

    // It finishes inside the grace.
    assert!(rig.resolver.document_mut(1).state.apply(cmd));
    ticket.finish();
    ack.send(ApplyAck {
        applied: true,
        ..ApplyAck::default()
    })
    .expect("caller still waiting");
    let response = call.join().expect("call");

    assert!(!is_error(&response), "{response}");
    assert_eq!(rig.page_count(1), 2);
    assert_eq!(rig.log_len(1), 1, "an applied write is remembered");
}

/// A call whose apply the UI thread started and never answered: the caller
/// has reported `OutcomeUnknown`, and the started change has since landed.
fn outcome_unknown_rig() -> (Rig, String) {
    let mut rig = Rig::single();
    let call = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let UiRequest::Apply {
        cmd, ticket, ack, ..
    } = rig.serve_until("apply")
    else {
        panic!("apply request");
    };
    assert!(ticket.try_start(rig.clock.now()));
    rig.clock.advance(BUDGET_OVER);
    wait_for_grace(&rig);
    // No answer inside the grace (the answer channel stays open: it is the
    // UI thread being slow, not gone).
    let response = rig.expire(call, STARTED_GRACE + Duration::from_millis(1));
    // The started apply lands afterwards, as it would on the real UI thread.
    assert!(rig.resolver.document_mut(1).state.apply(cmd));
    ticket.finish();
    let _ = ack.send(ApplyAck {
        applied: true,
        ..ApplyAck::default()
    });
    (rig, response)
}

#[test]
fn started_request_without_ack_reports_outcome_unknown_and_records_in_doubt() {
    let (rig, response) = outcome_unknown_rig();

    assert!(is_error(&response), "{response}");
    assert_eq!(error_text(&response), OUTCOME_UNKNOWN);
    assert_eq!(
        rig.log_len(1),
        1,
        "the doubt is recorded against the operationId"
    );
}

#[test]
fn retry_after_outcome_unknown_returns_outcome_unknown_not_a_second_apply() {
    let (mut rig, _) = outcome_unknown_rig();
    assert_eq!(rig.page_count(1), 2, "the started write did land");

    // The agent retries with the same operationId and arguments.
    let retry = rig.call(&add_page_call(2, 1, "op-a", "Landing"));
    let response = rig.serve_to_the_end(retry);

    assert!(is_error(&response), "{response}");
    assert_eq!(error_text(&response), OUTCOME_UNKNOWN);
    assert!(
        rig.queue.try_recv().is_err(),
        "a doubted operation never reaches the UI thread again"
    );
    assert_eq!(rig.page_count(1), 2, "not applied twice");

    // Same id, different arguments is still a client bug, not a retry.
    let conflicting = rig.call(&add_page_call(3, 1, "op-a", "Pricing"));
    let response = rig.serve_to_the_end(conflicting);
    assert!(error_text(&response).contains("already used for a different call"));

    // After re-reading, a new operationId is an ordinary new call.
    let fresh = rig.call(&add_page_call(4, 1, "op-b", "Pricing"));
    let response = rig.serve_to_the_end(fresh);
    assert!(!is_error(&response), "{response}");
    assert_eq!(rig.page_count(1), 3);
}

// ── retries after a call that never ran ─────────────────────────────────

#[test]
fn retry_after_host_busy_with_same_operation_id_applies_exactly_once() {
    let mut rig = Rig::single();
    let first = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let stale = rig.serve_until("apply");
    rig.clock.advance(BUDGET_OVER);
    assert_eq!(error_text(&first.join().expect("call")), HOST_BUSY);

    // The agent retries; the editor is responsive now.
    let retry = rig.call(&add_page_call(2, 1, "op-a", "Landing"));
    let response = rig.serve_to_the_end(retry);
    assert!(!is_error(&response), "{response}");
    assert_eq!(rig.page_count(1), 2);
    assert_eq!(
        json_body(&response)["result"].get("_meta"),
        None,
        "a real run, not a replay"
    );

    // The first, dead request finally reaches the pump: still nothing.
    rig.deliver(stale);
    assert_eq!(rig.page_count(1), 2, "exactly once");
    assert_eq!(rig.server.expired_requests_dropped(), 1);
}

#[test]
fn cancelled_call_does_not_poison_its_operation_id() {
    let mut rig = Rig::single();
    let first = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let stale = rig.serve_until("apply");
    rig.clock.advance(BUDGET_OVER);
    first.join().expect("call");
    assert_eq!(rig.log_len(1), 0, "a call that never ran is not remembered");
    rig.deliver(stale);

    // Same operationId, same arguments: allowed to run normally.
    let retry = rig.call(&add_page_call(2, 1, "op-a", "Landing"));
    let response = rig.serve_to_the_end(retry);
    assert!(!is_error(&response), "{response}");
    assert_eq!(rig.log_len(1), 1, "now the applied call is remembered");

    // ...and a third attempt is a replay of that applied call.
    let again = rig.call(&add_page_call(3, 1, "op-a", "Landing"));
    let replayed = json_body(&rig.serve_to_the_end(again));
    assert_eq!(replayed["result"]["_meta"]["replayed"], true);
    assert_eq!(rig.page_count(1), 2);
}

// ── locks and the next call ─────────────────────────────────────────────

#[test]
fn document_lock_is_released_after_a_timeout() {
    let mut rig = Rig::single();
    let gate = rig.documents.gate(DocumentId(1));

    // Timed out waiting for the snapshot.
    let call = rig.call(&add_page_call(1, 1, "op-a", "Landing"));
    let stale_snapshot = rig.serve_until("snapshot");
    rig.clock.advance(BUDGET_OVER);
    assert_eq!(error_text(&call.join().expect("call")), HOST_BUSY);
    assert!(
        gate.try_lock().is_ok(),
        "gate free after a snapshot timeout"
    );
    rig.deliver(stale_snapshot);

    // Timed out waiting for the apply.
    let call = rig.call(&add_page_call(2, 1, "op-b", "Landing"));
    let stale_apply = rig.serve_until("apply");
    rig.clock.advance(BUDGET_OVER);
    assert_eq!(error_text(&call.join().expect("call")), HOST_BUSY);
    assert!(gate.try_lock().is_ok(), "gate free after an apply timeout");
    rig.deliver(stale_apply);
    assert_eq!(rig.page_count(1), 1);
}

#[test]
fn document_lock_is_released_when_a_call_panics() {
    let documents = LiveDocuments::default();
    let gate = documents.gate(DocumentId(1));
    let for_thread = Arc::clone(&gate);
    let unwound = thread::spawn(move || {
        let _held = lock_gate(&for_thread);
        panic!("connection thread dies while holding the document's gate");
    })
    .join();
    assert!(unwound.is_err());

    // The next call to that document is not wedged by the poisoned mutex.
    let _next = lock_gate(&gate);
}

#[test]
fn next_call_to_the_same_document_succeeds_after_a_dropped_request() {
    let mut rig = Rig::single();
    let call = rig.call(&tool_call(1, "get_document_info", &document_arg(1)));
    let stale = rig.serve_until("snapshot");
    rig.clock.advance(BUDGET_OVER);
    assert_eq!(error_text(&call.join().expect("call")), HOST_BUSY);
    rig.deliver(stale);
    assert_eq!(rig.server.expired_requests_dropped(), 1);

    let next = rig.call(&tool_call(2, "get_document_info", &document_arg(1)));
    let response = rig.serve_to_the_end(next);

    assert!(!is_error(&response), "{response}");
}

#[test]
fn calls_to_other_documents_are_unaffected_by_one_documents_stuck_ui() {
    let mut resolver = TestResolver::with_ids(&[(3, true), (4, false)]);
    resolver.lend_parked = true;
    let mut rig = Rig::new(resolver);
    // d3's call goes first and its snapshot is held: d3's gate is provably
    // taken before d4's call starts.
    let stuck = rig.call(&add_page_call(1, 3, "op-a", "Landing"));
    let mut held = vec![rig.serve_until("snapshot")];
    assert_eq!(document_of(&held[0]), Some(DocumentId(3)));
    let other = rig.call(&add_page_call(2, 4, "op-b", "Pricing"));

    // The UI answers everything except d3's snapshot and apply.
    let patience = std::time::Instant::now() + Duration::from_secs(10);
    while !other.is_finished() {
        assert!(std::time::Instant::now() < patience, "d4's call was wedged");
        if let Ok(request) = rig.queue.recv_timeout(Duration::from_millis(10)) {
            if document_of(&request) == Some(DocumentId(3)) {
                held.push(request);
            } else {
                rig.deliver(request);
            }
        }
    }
    let response = other.join().expect("call");

    assert!(!is_error(&response), "{response}");
    assert_eq!(rig.page_count(4), 2, "d4's write landed");
    assert!(
        !stuck.is_finished(),
        "d3's call is still waiting for its UI thread"
    );

    rig.clock.advance(BUDGET_OVER);
    assert_eq!(error_text(&stuck.join().expect("call")), HOST_BUSY);
    for request in held {
        rig.deliver(request);
    }
    assert_eq!(rig.page_count(3), 1, "d3's abandoned write never lands");
}

// ── create_document ─────────────────────────────────────────────────────

#[test]
fn create_document_deadline_drops_the_write_before_any_file_is_created() {
    let root = std::fs::canonicalize(std::env::temp_dir())
        .expect("temp dir")
        .join(format!("openpencil-deadline-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("project root");
    let mut state = EditorState::starter();
    state.mark_document_changed();
    let mut rig = Rig::new(TestResolver::single(state));
    let arguments = serde_json::json!({"projectRoot": root.to_string_lossy(), "name": "landing"});
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{{"name":"create_document","arguments":{arguments}}}}}"#
    );

    let call = rig.call(&body);
    let stale = rig.serve_until("create-document");
    rig.clock.advance(BUDGET_OVER);
    let response = call.join().expect("call");
    rig.deliver(stale);

    let designs = root.join("designs");
    let created = designs.exists();
    let bound = rig.created.len();
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(error_text(&response), HOST_BUSY);
    assert!(
        !created,
        "no file or folder may be created for an expired call"
    );
    assert_eq!(bound, 0, "the host is never asked to bind a file");
    assert_eq!(rig.server.expired_requests_dropped(), 1);
}

#[test]
fn expired_whole_document_sync_is_dropped_by_the_pump_and_never_replaces_the_document() {
    let clock = TestClock::new();
    let (mut server, queue) = server_with_timing(clock.timing());
    let mut resolver = TestResolver::single(EditorState::new());
    let replacement = op_pen_loader::load_canonical(
        r#"{"version":"1.0","children":[],"pages":[{"id":"p1","name":"One","children":[]},{"id":"p2","name":"Two","children":[]}] }"#,
    )
    .expect("two-page replacement")
    .value;
    let (ack, ack_rx) = mpsc::sync_channel(1);
    queue
        .send(UiRequest::ReplaceDocument {
            doc: Box::new(replacement),
            editor_meta: op_pen_loader::EditorMeta::default(),
            ticket: Ticket::new(clock.now() + Duration::from_secs(5)),
            ack,
        })
        .expect("queue replace");
    clock.advance(BUDGET_OVER);

    let outcome = server.pump(&mut resolver);

    assert!(outcome.replaced.is_empty());
    assert_eq!(
        op_mcp::list_pages_snapshot(&resolver.document_mut(1).state).page_count,
        1
    );
    assert_eq!(server.expired_requests_dropped(), 1);
    assert!(matches!(
        ack_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}
