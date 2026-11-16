//! Document ids carry a per-launch nonce (`d<N>.<nonce>`), so an id kept from
//! an earlier launch can never silently reach whatever document is `d<N>` now.

use super::test_support::*;
use super::*;
use crate::mcp_serve::document_nonce::{launch_nonce, LaunchNonce};

fn info_call(arguments: &str) -> String {
    tool_call(1, "get_document_info", arguments)
}

fn posted(path: &str, body: &str, resolver: TestResolver) -> Posted {
    post(path, "", body, &LiveDocuments::default(), resolver, None)
}

/// A nonce that is not this launch's.
fn other_nonce() -> LaunchNonce {
    let launch = launch_nonce();
    ["ffffff", "000000"]
        .into_iter()
        .filter_map(LaunchNonce::parse)
        .find(|candidate| *candidate != launch)
        .expect("one of the two differs")
}

#[test]
fn bare_document_id_is_accepted_only_for_a_single_document() {
    // One document open and the bare id names it: honoured (old clients).
    let only = posted(
        "/mcp",
        &info_call(r#""documentId":"d1""#),
        TestResolver::single(EditorState::new()),
    );
    assert!(!is_error(&only.response), "{}", only.response);

    // One document open but the bare id names ANOTHER number: the id is from
    // a previous session, not an alias for "whatever is open".
    let mut resolver = TestResolver::with_ids(&[(3, true)]);
    resolver.lend_parked = true;
    let renumbered = posted("/mcp", &info_call(r#""documentId":"d1""#), resolver);
    assert!(is_error(&renumbered.response), "{}", renumbered.response);
    assert!(error_text(&renumbered.response).contains("previous session"));

    // Several documents open: even a bare id that matches one is refused,
    // because after a restart `d3` may be a different document.
    let several = posted(
        "/mcp",
        &info_call(r#""documentId":"d3""#),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    assert!(is_error(&several.response), "{}", several.response);
    assert_eq!(
        several.ui.seen,
        ["documents"],
        "nothing was read or applied"
    );

    // A bare route is held to the same rule.
    let bare_route = posted(
        "/mcp/d/d3",
        &info_call(""),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    assert!(is_error(&bare_route.response), "{}", bare_route.response);
}

#[test]
fn stale_nonce_id_is_refused_with_the_open_documents_listed() {
    let stale = format!("d3.{}", other_nonce());
    let refused = posted(
        "/mcp",
        &info_call(&format!(r#""documentId":"{stale}""#)),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );

    assert!(is_error(&refused.response), "{}", refused.response);
    let text = error_text(&refused.response);
    assert!(text.contains(&format!("'{stale}'")), "{text}");
    assert!(text.contains("previous session or is incomplete"), "{text}");
    assert!(
        text.contains(&format!("{} proj/doc3.op (active)", full(3))),
        "{text}"
    );
    assert!(
        text.contains(&format!("{} proj/doc4.op", full(4))),
        "{text}"
    );
    assert_eq!(refused.ui.seen, ["documents"], "refused before any read");

    // The same refusal on a route bound to a stale id.
    let route = format!("/mcp/d/{stale}");
    let bound = posted(
        &route,
        &info_call(""),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    assert!(is_error(&bound.response), "{}", bound.response);
    assert!(error_text(&bound.response).contains("previous session"));
    // ... but the catalog is not about a document and stays readable.
    let list = posted(
        &route,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    assert!(!is_error(&list.response), "{}", list.response);
}

#[test]
fn route_with_full_id_works() {
    let resolver = || TestResolver::with_ids(&[(3, true), (4, false)]);
    let reached = posted(&format!("/mcp/d/{}", full(3)), &info_call(""), resolver());
    assert!(!is_error(&reached.response), "{}", reached.response);
    assert_eq!(reached.ui.seen, ["snapshot"], "a full id needs no listing");

    // The argument form of the same id.
    let by_argument = posted("/mcp", &info_call(&document_arg(3)), resolver());
    assert!(!is_error(&by_argument.response), "{}", by_argument.response);

    // A full id of a document that is not open is "unknown", not "stale".
    let closed = posted("/mcp", &info_call(&document_arg(9)), resolver());
    assert!(error_text(&closed.response).contains("unknown documentId"));
}

#[test]
fn list_documents_reports_the_full_id_to_copy() {
    let listed = posted(
        "/mcp",
        &tool_call(1, "list_documents", ""),
        TestResolver::with_ids(&[(3, true), (4, false)]),
    );
    let rows: serde_json::Value =
        serde_json::from_str(&crate::mcp_serve::tool_text(&listed.response)).expect("array");
    assert_eq!(rows[0]["documentId"], full(3));
    assert_eq!(rows[1]["documentId"], full(4));
}

#[test]
fn nonce_changes_between_launches() {
    // Two servers of one process stand in for two launches.
    let first = other_nonce();
    let second = launch_nonce();
    assert_ne!(first, second);

    let (tx, rx) = std::sync::mpsc::channel::<UiRequest>();
    let ui = spawn_ui(rx, TestResolver::with_ids(&[(3, true), (4, false)]), None);
    let documents = LiveDocuments::default();
    let launch_one = UiSender::new(tx.clone(), LiveTiming::system()).with_nonce(first);
    let launch_two = UiSender::new(tx.clone(), LiveTiming::system()).with_nonce(second);

    // Launch one lists `d3.<first>`; the agent keeps that id.
    let listing = serve_one_via(
        "/mcp",
        "",
        &tool_call(1, "list_documents", ""),
        &documents,
        &launch_one,
    );
    let rows: serde_json::Value =
        serde_json::from_str(&crate::mcp_serve::tool_text(&listing)).expect("array");
    let kept_id = rows[0]["documentId"].as_str().expect("id").to_string();
    assert_eq!(kept_id, format!("d3.{first}"));

    // Launch two has a `d3` too, but it refuses the kept id.
    let after_restart = serve_one_via(
        "/mcp",
        "",
        &info_call(&format!(r#""documentId":"{kept_id}""#)),
        &documents,
        &launch_two,
    );
    assert!(is_error(&after_restart), "{after_restart}");
    let text = error_text(&after_restart);
    assert!(text.contains("previous session"), "{text}");
    assert!(text.contains(&format!("d3.{second}")), "{text}");
    drop((launch_one, launch_two, tx));
    ui.join().expect("ui");
}

#[test]
fn a_started_server_names_documents_with_the_injected_nonce() {
    let nonce = LaunchNonce::parse("a1b2c3").expect("nonce");
    let server = McpLiveServer::start_with_options(
        0,
        || {},
        LiveOptions {
            nonce: Some(nonce),
            ..LiveOptions::default()
        },
    )
    .expect("start on an ephemeral port");
    assert_eq!(server.nonce(), nonce);
    let default = McpLiveServer::start_with_wake(0, || {}).expect("start");
    assert_eq!(default.nonce(), launch_nonce());
}
