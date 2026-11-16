//! Routing, isolation and concurrency of the live MCP server across several
//! open documents of several projects, over real HTTP: a real TCP client
//! talks to the real `McpLiveServer`, and the test thread answers it through
//! `DesktopApp::poll_mcp_server`. Tab `a` and `b` belong to project `p1`, tab
//! `c` to `p2`; `c` is on screen, `a` and `b` are parked.

use super::multi_document_e2e_support::*;
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};

/// A client job making `count` sequential edits that each move the frame to
/// `(base + i, base + i)`, addressed with `address` (a route, plus an optional
/// `documentId` argument).
fn edit_job(
    path: String,
    document_argument: Option<String>,
    base: i64,
    count: i64,
    start: Arc<Barrier>,
) -> Job<Vec<Result<Reply, String>>> {
    Box::new(move |port| {
        start.wait();
        (0..count)
            .map(|i| {
                let mut arguments = move_frame(base + i, base + i);
                if let Some(document) = &document_argument {
                    arguments["documentId"] = json!(document);
                }
                post_tool(port, &path, "update_node", arguments)
            })
            .collect()
    })
}

#[test]
fn list_documents_over_http_reports_every_tab_with_path_project_and_active() {
    let mut fx = Fixture::new("list");

    let reply = fx.call("/mcp", "list_documents", json!({}));

    assert!(!reply.is_error(), "{}", reply.text());
    let rows = reply.json();
    let rows = rows.as_array().expect("an array of documents");
    assert_eq!(rows.len(), 3);
    let expected = [
        (fx.a, &fx.path_a, "a.op", "p1", false),
        (fx.b, &fx.path_b, "b.op", "p1", false),
        (fx.c, &fx.path_c, "c.op", "p2", true),
    ];
    for (row, (id, path, label, project, active)) in rows.iter().zip(expected) {
        assert_eq!(row["documentId"], Fixture::name(id).as_str(), "tab order");
        assert_eq!(row["path"], path.to_string_lossy().as_ref());
        assert_eq!(row["label"], label);
        assert_eq!(row["project"], project);
        assert_eq!(row["active"], active, "{label}");
        assert_eq!(row["dirty"], false);
        assert_eq!(row["status"], "idle");
    }
    // The same answer on a document-bound endpoint (the listing is global).
    let bound = fx.call(&Fixture::route(fx.a), "list_documents", json!({}));
    assert_eq!(bound.json(), reply.json());
}

#[test]
fn call_without_document_id_is_refused_with_a_listing_when_several_are_open() {
    let mut fx = Fixture::new("refuse");
    let before = [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id));

    let reply = fx.call("/mcp", "update_node", move_frame(9, 9));

    assert!(reply.is_error(), "an ambiguous write must be refused");
    let text = reply.text();
    assert!(text.contains("documentId"), "{text}");
    // Document ids are process-unique, so the expected line is built from them.
    for (id, line) in [
        (fx.a, "p1/a.op"),
        (fx.b, "p1/b.op"),
        (fx.c, "p2/c.op (active)"),
    ] {
        let listed = format!("{} {line}", Fixture::name(id));
        assert!(text.contains(&listed), "{listed} missing from: {text}");
    }
    let after = [fx.a, fx.b, fx.c].map(|id| fx.fingerprint(id));
    assert_eq!(before, after, "a refused call changes nothing");
    assert!(![fx.a, fx.b, fx.c].into_iter().any(|id| fx.is_dirty(id)));
}

#[test]
fn single_open_document_is_auto_selected() {
    let mut fx = Fixture::new("single");
    // Clean tabs close without a dialog. `a` and `b` are parked at 0 and 1.
    assert!(fx.app.close_document_tab(0));
    assert!(fx.app.close_document_tab(0));
    assert_eq!(fx.app.documents.len(), 1);

    let reply = fx.call("/mcp", "update_node", move_frame(31, 32));

    assert!(!reply.is_error(), "{}", reply.text());
    assert_eq!((fx.frame_x(fx.c), fx.frame_y(fx.c)), (31.0, 32.0));
    assert!(fx.is_dirty(fx.c));
    // A closed document's id is refused, with the remaining one listed.
    let stale = fx.call(
        "/mcp",
        "update_node",
        json!({"nodeId": FRAME_ID, "data": {"x": 1}, "documentId": Fixture::name(fx.a)}),
    );
    assert!(stale.is_error());
    assert!(
        stale.text().contains("unknown documentId"),
        "{}",
        stale.text()
    );
    assert_eq!(fx.frame_x(fx.c), 31.0, "the refusal changed nothing");
}

#[test]
fn edit_through_document_id_lands_only_in_that_parked_document() {
    let mut fx = Fixture::new("parked-edit");
    let [before_b, before_c] = [fx.b, fx.c].map(|id| fx.fingerprint(id));
    let bytes = [&fx.path_a, &fx.path_b, &fx.path_c].map(|path| read_bytes(path));
    let mut arguments = move_frame(40, 41);
    arguments["documentId"] = json!(Fixture::name(fx.a));

    let reply = fx.call("/mcp", "update_node", arguments);

    assert!(!reply.is_error(), "{}", reply.text());
    assert_eq!((fx.frame_x(fx.a), fx.frame_y(fx.a)), (40.0, 41.0));
    assert!(fx.is_dirty(fx.a), "a holds the unsaved edit");
    assert_eq!(fx.fingerprint(fx.c), before_c, "c is byte-identical");
    assert_eq!(fx.fingerprint(fx.b), before_b, "b is byte-identical");
    assert!(!fx.is_dirty(fx.c) && !fx.is_dirty(fx.b));
    assert_eq!(fx.app.documents.active_id(), fx.c, "no tab switch happened");
    let after = [&fx.path_a, &fx.path_b, &fx.path_c].map(|path| read_bytes(path));
    assert_eq!(bytes, after, "an MCP edit never writes the files");
}

#[test]
fn route_bound_endpoint_edits_its_own_document_even_after_a_tab_switch() {
    let mut fx = Fixture::new("route-switch");
    let route = Fixture::route(fx.a);
    // `a` is on screen for the first edit.
    assert!(fx.app.activate_document_slot(0));
    assert_eq!(fx.app.documents.active_id(), fx.a);
    let first = fx.call(&route, "update_node", move_frame(11, 11));
    assert!(!first.is_error(), "{}", first.text());
    assert_eq!(fx.frame_x(fx.a), 11.0);

    // The user moves to `c` mid-session; `a` is now parked.
    assert!(fx.app.activate_document_slot(2));
    assert_eq!(fx.app.documents.active_id(), fx.c);
    let before_c = fx.fingerprint(fx.c);
    let second = fx.call(&route, "update_node", move_frame(22, 23));

    assert!(!second.is_error(), "{}", second.text());
    assert_eq!((fx.frame_x(fx.a), fx.frame_y(fx.a)), (22.0, 23.0));
    assert_eq!(
        fx.fingerprint(fx.c),
        before_c,
        "the on-screen c was not hit"
    );
    assert!(!fx.is_dirty(fx.c));
    // A `documentId` that disagrees with the bound route is refused, not
    // preferred, and nothing moves.
    let mut conflicting = move_frame(99, 99);
    conflicting["documentId"] = json!(Fixture::name(fx.c));
    let conflict = fx.call(&route, "update_node", conflicting);
    assert!(conflict.is_error());
    assert!(conflict.text().contains("conflicts"), "{}", conflict.text());
    assert_eq!(fx.frame_x(fx.a), 22.0);
    assert_eq!(fx.fingerprint(fx.c), before_c);
    // The catalog of a parked document's endpoint is still readable.
    let catalog_route = route.clone();
    let mut list = fx.drive(vec![Box::new(move |port| {
        post_raw(
            port,
            &catalog_route,
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#,
        )
    }) as Job<Result<Reply, String>>]);
    let list = list.remove(0).expect("HTTP exchange");
    assert!(!list.is_error(), "{}", list.text());
    assert!(list.body["result"]["tools"].is_array());
}

#[test]
fn two_clients_editing_two_documents_concurrently_do_not_interfere() {
    let mut fx = Fixture::new("concurrent");
    let before_c = fx.fingerprint(fx.c);
    let start = Arc::new(Barrier::new(2));
    // Client one is bound to `a` by its route; client two names `b` per call.
    let jobs = vec![
        edit_job(Fixture::route(fx.a), None, 100, 20, Arc::clone(&start)),
        edit_job("/mcp".into(), Some(Fixture::name(fx.b)), 200, 20, start),
    ];

    let results = fx.drive(jobs);

    for (client, replies) in results.iter().enumerate() {
        assert_eq!(replies.len(), 20);
        for reply in replies {
            let reply = reply.as_ref().expect("HTTP exchange");
            assert!(!reply.is_error(), "client {client}: {}", reply.text());
        }
    }
    assert_eq!((fx.frame_x(fx.a), fx.frame_y(fx.a)), (119.0, 119.0));
    assert_eq!((fx.frame_x(fx.b), fx.frame_y(fx.b)), (219.0, 219.0));
    assert_eq!(
        fx.fingerprint(fx.c),
        before_c,
        "the third document is untouched"
    );
    assert!(fx.is_dirty(fx.a) && fx.is_dirty(fx.b) && !fx.is_dirty(fx.c));
}

#[test]
fn two_clients_editing_the_same_document_are_serialized() {
    let mut fx = Fixture::new("same-doc");
    let before_c = fx.fingerprint(fx.c);
    let start = Arc::new(Barrier::new(2));
    let jobs = vec![
        edit_job(Fixture::route(fx.b), None, 1000, 20, Arc::clone(&start)),
        edit_job("/mcp".into(), Some(Fixture::name(fx.b)), 2000, 20, start),
    ];

    let results = fx.drive(jobs);

    // No spurious `document changed` refusal for an ordinary edit.
    for replies in &results {
        for reply in replies {
            let reply = reply.as_ref().expect("HTTP exchange");
            assert!(!reply.is_error(), "{}", reply.text());
        }
    }
    // Every write was applied whole (x and y always move together), and the
    // last write of the client that finished last is what remains.
    let (x, y) = (fx.frame_x(fx.b), fx.frame_y(fx.b));
    assert_eq!(x, y, "a torn write would leave x and y apart");
    assert!(
        x == 1019.0 || x == 2019.0,
        "final x {x} is not a last write"
    );
    // The document is still valid JSON that reloads.
    let json = fx.fingerprint(fx.b);
    assert!(serde_json::from_str::<Value>(&json).is_ok());
    assert_eq!(fx.fingerprint(fx.c), before_c);
}
