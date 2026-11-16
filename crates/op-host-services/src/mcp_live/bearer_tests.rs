//! The opt-in bearer token (`mcp_require_token`) and the request-size caps,
//! driven through the real `serve_connection` router: a refused request must
//! never reach the UI thread.

use super::test_support::*;
use super::*;

const PORT: u16 = 51236;
const INSTANCE_TOKEN: &str = "instance-token-0001";
const BEARER: &str = "9f2c0a77d1b34e58a6c5f0e2b8d4713c9f2c0a77d1b34e58a6c5f0e2b8d4713c";

fn call_body() -> String {
    tool_call(11, "list_pages", &document_arg(1))
}

const PING: &str = r#"{"jsonrpc":"2.0","id":5,"method":"ping"}"#;

fn shutdown_body(token: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"openpencil/shutdown","params":{{"token":"{token}"}}}}"#
    )
}

struct Driven {
    response: String,
    /// What reached the UI thread.
    reached_ui: bool,
    quit: bool,
}

/// One request through the router, with the bearer requirement set to
/// `required` and a UI thread that answers `list_pages`.
fn drive(
    required: Option<&str>,
    method: &str,
    path: &str,
    extra_headers: &str,
    body: &str,
) -> Driven {
    let (req_tx, req_rx) = mpsc::channel::<UiRequest>();
    let responder = thread::spawn(
        move || match req_rx.recv_timeout(Duration::from_millis(300)) {
            Ok(UiRequest::ListPages { ack, .. }) => {
                let _ = ack.send(Ok(op_mcp::ListPages {
                    page_count: 1,
                    active_page_index: 0,
                    pages: vec![("p1".to_string(), "One".to_string())],
                }));
                true
            }
            Ok(_) => true,
            Err(_) => false,
        },
    );
    let admission = LiveAdmission::new(INSTANCE_TOKEN.to_string(), PORT);
    admission.set_bearer(required.map(str::to_string));
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let documents = LiveDocuments::default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = MockStream::new(request);
    serve_connection(
        &mut stream,
        &UiSender::from(req_tx),
        &admission,
        &documents,
        &quit_flag,
        &wake_ui,
        &client_identity,
    )
    .expect("a refusal is answered on the wire");
    Driven {
        response: stream.response(),
        reached_ui: responder.join().expect("responder"),
        quit: quit_flag.load(Ordering::Acquire),
    }
}

fn bearer(token: &str) -> String {
    format!("Authorization: Bearer {token}\r\n")
}

#[test]
fn token_required_rejects_requests_without_or_with_a_wrong_token() {
    for (label, headers) in [
        ("no header", String::new()),
        ("wrong token", bearer("not-the-token")),
        ("a prefix of the token", bearer(&BEARER[..BEARER.len() - 1])),
        ("the token plus a byte", bearer(&format!("{BEARER}0"))),
        (
            "the wrong scheme",
            format!("Authorization: Basic {BEARER}\r\n"),
        ),
        (
            "the legacy header",
            format!("X-OpenPencil-Token: {INSTANCE_TOKEN}\r\n"),
        ),
        ("an empty bearer", "Authorization: Bearer \r\n".to_string()),
    ] {
        let driven = drive(Some(BEARER), "POST", "/mcp", &headers, &call_body());
        assert!(
            driven.response.starts_with("HTTP/1.1 401"),
            "{label}: {}",
            driven.response
        );
        assert!(
            driven.response.contains("-32001"),
            "{label}: {}",
            driven.response
        );
        assert!(
            driven.response.contains(r#""id":11"#),
            "echoes the id: {}",
            driven.response
        );
        assert!(
            !driven.reached_ui,
            "{label}: a refused request must not reach the UI thread"
        );
    }

    // Every route but the probes: initialize, tools/list, the REST sync.
    let initialize = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
    let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    for body in [initialize, list] {
        let driven = drive(Some(BEARER), "POST", "/mcp", "", body);
        assert!(
            driven.response.starts_with("HTTP/1.1 401"),
            "{body}: {}",
            driven.response
        );
    }
    let sync = drive(
        Some(BEARER),
        "POST",
        "/api/mcp/document",
        "Content-Type: application/json\r\n",
        r#"{"document":{}}"#,
    );
    assert!(
        sync.response.starts_with("HTTP/1.1 401"),
        "{}",
        sync.response
    );
    assert!(!sync.reached_ui);

    // The right token is served, with the scheme in any case.
    for header in [
        bearer(BEARER),
        format!("authorization: bearer {BEARER}\r\n"),
    ] {
        let driven = drive(Some(BEARER), "POST", "/mcp", &header, &call_body());
        assert!(
            driven.response.starts_with("HTTP/1.1 200"),
            "{}",
            driven.response
        );
        assert!(driven.response.contains("pageCount"), "{}", driven.response);
        assert!(driven.reached_ui);
    }
    let initialized = drive(Some(BEARER), "POST", "/mcp", &bearer(BEARER), initialize);
    assert!(
        initialized.response.starts_with("HTTP/1.1 200"),
        "{}",
        initialized.response
    );
}

#[test]
fn flag_off_keeps_todays_open_access() {
    let tokenless = drive(None, "POST", "/mcp", "", &call_body());
    assert!(
        tokenless.response.starts_with("HTTP/1.1 200"),
        "{}",
        tokenless.response
    );
    assert!(tokenless.reached_ui);
    // A stray Authorization header is simply ignored while the flag is off.
    let stray = drive(None, "POST", "/mcp", &bearer("whatever"), &call_body());
    assert!(
        stray.response.starts_with("HTTP/1.1 200"),
        "{}",
        stray.response
    );
    // An empty token is the same as no token: it cannot lock everyone out.
    let empty = drive(Some(""), "POST", "/mcp", "", &call_body());
    assert!(
        empty.response.starts_with("HTTP/1.1 200"),
        "{}",
        empty.response
    );
}

#[test]
fn ping_does_not_leak_the_shutdown_token_when_the_flag_is_on() {
    // Flag off: today's reply, token included (the CLI's identity check).
    let open = drive(None, "POST", "/mcp", "", PING);
    assert!(open.response.contains(INSTANCE_TOKEN), "{}", open.response);

    // Flag on, no credentials: liveness is answered, the token is not.
    let anonymous = drive(Some(BEARER), "POST", "/mcp", "", PING);
    assert!(
        anonymous.response.starts_with("HTTP/1.1 200"),
        "{}",
        anonymous.response
    );
    assert!(
        anonymous.response.contains(r#""mode":"live""#),
        "{}",
        anonymous.response
    );
    assert!(
        anonymous.response.contains(r#""authRequired":true"#),
        "{}",
        anonymous.response
    );
    assert!(
        !anonymous.response.contains(INSTANCE_TOKEN),
        "{}",
        anonymous.response
    );
    // A wrong bearer is no better than none.
    let wrong = drive(Some(BEARER), "POST", "/mcp", &bearer("nope"), PING);
    assert!(
        !wrong.response.contains(INSTANCE_TOKEN),
        "{}",
        wrong.response
    );
    // The unlock is the JSON-RPC route only: a REST body that merely says
    // "ping" gets nothing.
    let disguised = drive(
        Some(BEARER),
        "POST",
        "/api/mcp/document",
        "Content-Type: application/json\r\n",
        r#"{"method":"ping","id":1,"document":{}}"#,
    );
    assert!(
        disguised.response.starts_with("HTTP/1.1 401"),
        "{}",
        disguised.response
    );

    // An authenticated client may still verify the instance identity.
    let authed = drive(Some(BEARER), "POST", "/mcp", &bearer(BEARER), PING);
    assert!(
        authed.response.contains(INSTANCE_TOKEN),
        "{}",
        authed.response
    );
}

#[test]
fn the_shutdown_keeps_its_own_token_semantics_when_the_flag_is_on() {
    // `op stop` presents the instance token in the body, never a bearer.
    let stopped = drive(
        Some(BEARER),
        "POST",
        "/mcp",
        "",
        &shutdown_body(INSTANCE_TOKEN),
    );
    assert!(
        stopped.response.contains("shuttingDown"),
        "{}",
        stopped.response
    );
    assert!(stopped.quit);
    // A wrong body token with no bearer is refused and quits nothing.
    let refused = drive(Some(BEARER), "POST", "/mcp", "", &shutdown_body("guess"));
    assert!(
        refused.response.starts_with("HTTP/1.1 401"),
        "{}",
        refused.response
    );
    assert!(!refused.quit);
}

#[test]
fn a_browser_preflight_is_not_blocked_by_the_token() {
    let preflight = drive(Some(BEARER), "OPTIONS", "/mcp", "", "");
    assert!(
        preflight.response.starts_with("HTTP/1.1 204"),
        "{}",
        preflight.response
    );
}

#[test]
fn token_is_compared_in_constant_time() {
    // Structural: every comparison of a credential goes through the
    // constant-time helper (unit-tested in `mcp_serve::constant_time`), never
    // `==` on the secret.
    let admission = include_str!("admission.rs");
    assert!(admission.contains("secrets_equal"), "bearer check");
    let wire = include_str!("../mcp_serve/wire.rs");
    assert!(wire.contains("secrets_equal"), "shutdown token check");
    assert!(
        !wire.contains("req_token == token"),
        "the shutdown token must not be compared with =="
    );
    assert!(
        !admission.contains("== required") && !admission.contains("required =="),
        "the bearer token must not be compared with =="
    );
}

// ── request-size caps ───────────────────────────────────────────────────

/// A request that DECLARES `length` bytes and sends none: a cap that works
/// must answer before it tries to read them.
fn declared(path: &str, length: usize) -> String {
    format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
    )
}

fn serve_declared(path: &str, length: usize) -> (String, bool) {
    let (req_tx, req_rx) = mpsc::channel::<UiRequest>();
    let admission = LiveAdmission::new(INSTANCE_TOKEN.to_string(), PORT);
    let quit_flag = AtomicBool::new(false);
    let wake_ui: UiWake = Arc::new(|| {});
    let client_identity = Mutex::new(None);
    let documents = LiveDocuments::default();
    let mut stream = MockStream::new(declared(path, length));
    serve_connection(
        &mut stream,
        &UiSender::from(req_tx),
        &admission,
        &documents,
        &quit_flag,
        &wake_ui,
        &client_identity,
    )
    .expect("an oversized body is answered on the wire, not a server error");
    (stream.response(), req_rx.try_recv().is_ok())
}

#[test]
fn oversized_bodies_get_413() {
    use crate::mcp_serve::body_limits::{DOCUMENT_BODY_MAX, LIVE_TOOL_CALL_BODY_MAX};

    for path in ["/mcp", "/mcp/lean", "/"] {
        let (response, reached_ui) = serve_declared(path, LIVE_TOOL_CALL_BODY_MAX + 1);
        assert!(response.starts_with("HTTP/1.1 413"), "{path}: {response}");
        assert!(!reached_ui);
    }
    // A document may be far larger than a call, up to its own ceiling.
    let (response, _) = serve_declared("/api/mcp/document", DOCUMENT_BODY_MAX + 1);
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");
    // Just inside a call's cap is not refused for its size (this request then
    // fails for sending no body, which is a different, later error).
    let mut stream = MockStream::new(declared("/mcp", LIVE_TOOL_CALL_BODY_MAX));
    let read = crate::mcp_serve::read_http_request_limited(
        &mut stream,
        crate::mcp_serve::body_limits::BodyLimits::LIVE,
    );
    assert!(
        !matches!(read, Err(crate::mcp_serve::McpServeError::Framing { .. })),
        "{read:?}"
    );
}

// ── connection cap (the 256-entry operation log is pinned in operation_log_tests) ──

#[test]
fn the_live_server_sheds_connections_beyond_its_cap() {
    use std::io::Read;
    use std::net::TcpStream;

    let server = McpLiveServer::start_with_wake(0, || {}).expect("start");
    let address = ("127.0.0.1", server.port());
    // Idle connections hold their slots (the server waits for a request).
    let held: Vec<TcpStream> = (0..MAX_LIVE_CONNS)
        .map(|_| TcpStream::connect(address).expect("connect"))
        .collect();
    let mut shed = None;
    for _ in 0..100 {
        let mut extra = TcpStream::connect(address).expect("connect");
        extra
            .set_read_timeout(Some(Duration::from_millis(200)))
            .expect("timeout");
        let mut reply = String::new();
        let _ = extra.read_to_string(&mut reply);
        if reply.starts_with("HTTP/1.1 503") {
            shed = Some(reply);
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let reply = shed.expect("the connection over the cap is shed with a 503");
    assert!(reply.contains("server busy"), "{reply}");
    drop(held);
    drop(server);
}
