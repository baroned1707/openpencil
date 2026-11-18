use std::collections::BTreeMap;

use serde_json::Value;

use super::*;

const CAP: usize = 6 * 1024;

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    map.insert("category".into(), "data".into());
    map
}

fn data(tool: &GetGuidelines, pairs: &[(&str, &str)]) -> Value {
    match tool.call(&args(pairs)) {
        ToolOutcome::OkJson(text) => {
            assert!(text.len() <= CAP, "{} bytes", text.len());
            serde_json::from_str(&text).expect("the result is one JSON document")
        }
        other => panic!("expected OkJson, got {other:?}"),
    }
}

fn error_text(outcome: ToolOutcome) -> String {
    match outcome {
        ToolOutcome::Err(_, message) => message,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn get_guidelines_data_returns_capped_json() {
    let tool = get_guidelines_snapshot();
    let found = data(&tool, &[("query", "fintech dashboard")]);
    let matches = found["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 1, "limit defaults to 1");
    assert_eq!(matches[0]["id"], "fintech-dashboard");
    assert_eq!(matches[0]["source"]["licenseSpdx"], "MIT");
    assert!(matches[0]["palette"]["primary"].as_str().is_some());
    assert!(found["source"].as_str().is_some());

    // limit is capped at 3 and the text stays within 6 KB.
    let many = data(
        &tool,
        &[
            ("query", "fintech saas health shop docs portfolio"),
            ("limit", "99"),
        ],
    );
    assert_eq!(many["matches"].as_array().expect("matches").len(), 3);

    // platform narrows the search.
    let mobile = data(&tool, &[("query", "shop"), ("platform", "Mobile")]);
    assert_eq!(mobile["matches"][0]["id"], "ecommerce-storefront");
    let none = data(
        &tool,
        &[("query", "saas landing page"), ("platform", "mobile")],
    );
    assert_eq!(none["matches"], serde_json::json!([]));
}

#[test]
fn no_close_match_is_a_one_line_message() {
    let found = data(&get_guidelines_snapshot(), &[("query", "restaurant menu")]);
    assert_eq!(found["matches"], serde_json::json!([]));
    let message = found["message"].as_str().expect("message");
    assert!(message.contains("No close product type matched"));
    assert!(message.contains("guidelines"));
    assert!(!message.contains('\n'));
}

#[test]
fn data_arguments_are_validated() {
    let tool = get_guidelines_snapshot();
    assert!(error_text(tool.call(&args(&[]))).contains("query is required"));
    assert!(error_text(tool.call(&args(&[("query", "   ")]))).contains("query is required"));
    let platform = error_text(tool.call(&args(&[("query", "shop"), ("platform", "fridge")])));
    assert!(platform.contains("platform must be one of web, mobile"));
    for bad in ["0", "-1", "two", "1.5"] {
        let message = error_text(tool.call(&args(&[("query", "shop"), ("limit", bad)])));
        assert!(message.contains("limit must be"), "{bad}: {message}");
    }
    let unknown = error_text(tool.call(&BTreeMap::from([("category".into(), "nope".into())])));
    assert!(unknown.contains("\"data\""));
}

#[test]
fn guide_and_style_categories_are_unchanged_by_data() {
    // No category keeps meaning "guide": a query without a category must not
    // be routed to the data table.
    let tool = get_guidelines_snapshot();
    let outcome = tool.call(&BTreeMap::from([("query".into(), "fintech".into())]));
    assert!(error_text(outcome).contains("topic is required"));
}

#[test]
fn the_in_app_tool_serves_the_same_data_and_a_wire_number_limit_survives() {
    // The wire parser turns a JSON number into the flat string map the tool
    // reads, so `"limit": 2` (a number, as models often send) must work.
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_guidelines","arguments":{"category":"data","query":"fintech saas health","limit":2}}}"#;
    let call = crate::parse_tool_call(line).expect("wire call parses");
    assert_eq!(call.arguments.get("limit").map(String::as_str), Some("2"));
    let from_wire = match get_guidelines_in_app_snapshot().call(&call.arguments) {
        ToolOutcome::OkJson(text) => serde_json::from_str::<Value>(&text).expect("json"),
        other => panic!("expected OkJson, got {other:?}"),
    };
    assert_eq!(from_wire["matches"].as_array().expect("matches").len(), 2);

    // MCP and in-app surfaces return byte-identical data (no finalize text).
    let pairs = [("query", "ecommerce store")];
    assert_eq!(
        data(&get_guidelines_snapshot(), &pairs),
        data(&get_guidelines_in_app_snapshot(), &pairs)
    );
}
