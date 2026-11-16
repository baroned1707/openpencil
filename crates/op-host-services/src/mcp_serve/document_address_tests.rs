use super::*;

#[test]
fn ids_round_trip_and_only_the_canonical_spelling_parses() {
    assert_eq!(format_document_id(DocumentId(12)), "d12");
    assert_eq!(parse_document_id("d12"), Some(DocumentId(12)));
    for bad in ["12", "D12", "d", "d01", "d+1", "d-1", "d1 ", "", "dx"] {
        assert_eq!(parse_document_id(bad), None, "{bad:?}");
    }
}

#[test]
fn document_id_and_operation_id_are_stripped_before_dispatch() {
    let body = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"set_node_name","arguments":{"documentId":"d3","operationId":"op-1","node_id":"7","name":"documentId"}}}"#;
    let (address, stripped) = split_address(body);
    assert_eq!(address.document_id.as_deref(), Some("d3"));
    assert_eq!(address.operation_id.as_deref(), Some("op-1"));
    let call = op_mcp::parse_tool_call(&stripped).expect("still a tool call");
    assert_eq!(call.tool, "set_node_name");
    assert!(!call.arguments.contains_key("documentId"));
    assert!(!call.arguments.contains_key("operationId"));
    assert_eq!(call.arguments.get("node_id").map(String::as_str), Some("7"));
    // A value that merely spells the key name survives.
    assert_eq!(
        call.arguments.get("name").map(String::as_str),
        Some("documentId")
    );
}

#[test]
fn split_leaves_a_body_without_addressing_untouched() {
    let body =
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"undo","arguments":{}}}"#;
    let (address, stripped) = split_address(body);
    assert_eq!(address, CallAddress::default());
    assert!(matches!(stripped, Cow::Borrowed(_)));
}

#[test]
fn split_only_reads_the_arguments_object() {
    // A nested lookalike elsewhere in the envelope is not the argument.
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"undo","_meta":{"documentId":"d9"},"arguments":{}}}"#;
    let (address, stripped) = split_address(body);
    assert_eq!(address, CallAddress::default());
    assert!(matches!(stripped, Cow::Borrowed(_)));
}

#[test]
fn split_handles_the_legacy_direct_shape_and_blank_values() {
    let body = r#"{"id":2,"method":"get_node","params":{"documentId":"d2","operationId":"  ","node_id":"1"}}"#;
    let (address, stripped) = split_address(body);
    assert_eq!(address.document_id.as_deref(), Some("d2"));
    assert_eq!(address.operation_id, None);
    let call = op_mcp::parse_tool_call(&stripped).expect("call");
    assert_eq!(call.arguments.len(), 1);
}

#[test]
fn a_numeric_document_id_is_reported_as_the_wrong_value_it_is() {
    let body =
        r#"{"id":2,"method":"tools/call","params":{"name":"undo","arguments":{"documentId":3}}}"#;
    let (address, _) = split_address(body);
    assert_eq!(address.document_id.as_deref(), Some("3"));
}

#[test]
fn routes_decode_the_bound_document_and_catalog() {
    let full = parse_route("/mcp").expect("full");
    assert_eq!((full.catalog, full.document), (McpToolCatalog::Full, None));
    let lean = parse_route("/mcp/lean").expect("lean");
    assert_eq!(lean.catalog, McpToolCatalog::Lean);
    let bare = |id| {
        Some(DocumentRef {
            id: DocumentId(id),
            nonce: None,
        })
    };
    let bound = parse_route("/mcp/d/3").expect("bound");
    assert_eq!(
        (bound.catalog, bound.document),
        (McpToolCatalog::Full, bare(3))
    );
    let bound_lean = parse_route("/mcp/d/3/lean").expect("bound lean");
    assert_eq!(
        (bound_lean.catalog, bound_lean.document),
        (McpToolCatalog::Lean, bare(3))
    );
    assert_eq!(parse_route("/mcp/d/d3").and_then(|r| r.document), bare(3));
    for bad in [
        "/mcp/d/",
        "/mcp/d/03",
        "/mcp/d/d3.A1B2C3",
        "/mcp/d/d3.a1b2c",
        "/mcp/d/d3.a1b2c3d",
        "/mcp/d/d3.",
        "/mcp/d/.a1b2c3",
        "/mcp/d/x",
        "/mcp/d/3/tiny",
        "/mcp/d/3/lean/x",
        "/mcp/tiny",
    ] {
        assert!(parse_route(bad).is_none(), "{bad}");
    }
}

#[test]
fn mcp_url_for_document_formats_routes() {
    let nonce = launch_nonce();
    assert_eq!(
        mcp_url_for_document(3100, DocumentId(3), false),
        format!("http://127.0.0.1:3100/mcp/d/d3.{nonce}")
    );
    assert_eq!(
        mcp_url_for_document(3100, DocumentId(12), true),
        format!("http://127.0.0.1:3100/mcp/d/d12.{nonce}/lean")
    );
    // The URL parses back to the route it names, nonce included.
    let url = mcp_url_for_document(3100, DocumentId(12), true);
    let path = url.strip_prefix("http://127.0.0.1:3100").expect("path");
    let route = parse_route(path).expect("route");
    assert_eq!(
        route.document,
        Some(DocumentRef {
            id: DocumentId(12),
            nonce: Some(nonce)
        })
    );
    assert_eq!(route.catalog, McpToolCatalog::Lean);
}

#[test]
fn route_with_full_id_works() {
    let route = parse_route("/mcp/d/d3.a1b2c3").expect("full id route");
    assert_eq!(
        route.document,
        Some(DocumentRef {
            id: DocumentId(3),
            nonce: LaunchNonce::parse("a1b2c3")
        })
    );
    // The number alone, with a nonce, is also a route (the `d` is optional).
    assert_eq!(
        parse_route("/mcp/d/3.a1b2c3").and_then(|route| route.document),
        route.document
    );
}

#[test]
fn a_bare_and_a_full_spelling_agree_only_when_they_can_name_one_document() {
    let bare = parse_document_ref("d3").expect("bare");
    let full = parse_document_ref("d3.a1b2c3").expect("full");
    let other_launch = parse_document_ref("d3.ffffff").expect("full");
    assert!(bare.agrees_with(&full) && full.agrees_with(&bare));
    assert!(!full.agrees_with(&other_launch));
    assert!(!bare.agrees_with(&parse_document_ref("d4").expect("bare")));
    // The headless parser stays bare-only.
    assert_eq!(parse_document_id("d3"), Some(DocumentId(3)));
    assert_eq!(parse_document_id("d3.a1b2c3"), None);
}

#[test]
fn header_check_never_requires_the_header_but_enforces_it_when_sent() {
    assert_eq!(check_document_header(None, Some("d3")), Ok(()));
    assert_eq!(check_document_header(None, None), Ok(()));
    assert_eq!(check_document_header(Some("d3"), Some("d3")), Ok(()));
    assert_eq!(
        check_document_header(Some("d4"), Some("d3")),
        Err(HeaderMismatch::Differs {
            header: "d4".into(),
            body: Some("d3".into())
        })
    );
    assert!(matches!(
        check_document_header(Some("d3"), None),
        Err(HeaderMismatch::Differs { body: None, .. })
    ));
}

#[test]
fn header_check_decodes_the_base64_sentinel_and_rejects_bad_bytes() {
    // base64("d3") == "ZDM="
    assert_eq!(
        check_document_header(Some("=?base64?ZDM=?="), Some("d3")),
        Ok(())
    );
    assert_eq!(
        check_document_header(Some("=?base64?%%%?="), Some("d3")),
        Err(HeaderMismatch::Malformed)
    );
    assert_eq!(
        check_document_header(Some("d\u{7f}3"), Some("d3")),
        Err(HeaderMismatch::Malformed)
    );
}

#[test]
fn header_mismatch_body_is_a_json_rpc_error_with_the_spec_code() {
    let request = r#"{"jsonrpc":"2.0","id":"abc","method":"tools/call","params":{}}"#;
    let body = header_mismatch_body(request, &HeaderMismatch::Malformed);
    let value: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(value["error"]["code"], -32020);
    assert_eq!(value["id"], "abc");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .starts_with("HeaderMismatch"));
}

#[test]
fn headless_accepts_d1_or_nothing_and_refuses_other_documents() {
    let call = |args: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"undo","arguments":{{{args}}}}}}}"#
        )
    };
    assert!(matches!(
        headless_line(&call("")),
        HeadlessCall::Dispatch(_)
    ));
    let named = call(r#""documentId":"d1","operationId":"x""#);
    let HeadlessCall::Dispatch(accepted) = headless_line(&named) else {
        panic!("d1 is the headless document");
    };
    assert!(!accepted.contains("documentId"), "{accepted}");
    let other = call(r#""documentId":"d2""#);
    let HeadlessCall::Refuse(refusal) = headless_line(&other) else {
        panic!("d2 does not exist");
    };
    assert!(refusal.contains(r#""isError":true"#), "{refusal}");
    assert!(refusal.contains("exactly one document, d1"), "{refusal}");
}
