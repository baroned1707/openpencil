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
        &[("query", "saas landing page"), ("platform", "card")],
    );
    assert_eq!(none["matches"], serde_json::json!([]));
}

#[test]
fn no_close_match_is_a_one_line_message() {
    let found = data(&get_guidelines_snapshot(), &[("query", "hello world")]);
    assert_eq!(found["matches"], serde_json::json!([]));
    let message = found["message"].as_str().expect("message");
    assert!(message.contains("No close product type matched"));
    assert!(message.contains("guidelines"));
    assert!(!message.contains('\n'));
}

#[test]
fn get_guidelines_data_returns_an_imported_match_with_source_and_license() {
    // "restaurant menu" is a product type only the imported table has.
    let found = data(&get_guidelines_snapshot(), &[("query", "restaurant menu")]);
    let first = &found["matches"][0];
    assert_eq!(first["id"], "uiuxpm-restaurant-food-service");
    assert_eq!(first["source"]["name"], "UI UX Pro Max (MIT)");
    assert_eq!(first["source"]["licenseSpdx"], "MIT");
    assert_eq!(
        first["source"]["url"],
        "https://github.com/nextlevelbuilder/ui-ux-pro-max-skill"
    );
    let commit = first["source"]["commit"].as_str().expect("commit");
    assert!(commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(first["palette"]["primary"]
        .as_str()
        .expect("hex")
        .starts_with('#'));
    assert!(first["rules"]
        .as_array()
        .is_some_and(|rules| !rules.is_empty()));
    assert!(found["notice"]
        .as_str()
        .expect("notice")
        .contains("not instructions"));
    // The in-app surface serves the same imported record.
    assert_eq!(
        found,
        data(
            &get_guidelines_in_app_snapshot(),
            &[("query", "restaurant menu")]
        )
    );
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

// ---- kind: product | style -------------------------------------------------

/// Responses of the code BEFORE `kind` existed, captured byte for byte.
const BEFORE_SEED: &str =
    include_str!("../tests/fixtures/get_guidelines_data_fintech_dashboard_before_kind.json");
const BEFORE_SAAS: &str =
    include_str!("../tests/fixtures/get_guidelines_data_saas_general_before_kind.json");
const BEFORE_THREE: &str =
    include_str!("../tests/fixtures/get_guidelines_data_fintech_three_before_kind.json");

fn text_of(outcome: ToolOutcome) -> String {
    match outcome {
        ToolOutcome::OkJson(text) => text,
        other => panic!("expected OkJson, got {other:?}"),
    }
}

/// The response with every `styleSummaries` key removed.
fn without_summaries(text: &str) -> Value {
    let mut value: Value = serde_json::from_str(text).expect("json");
    for found in value["matches"].as_array_mut().expect("matches") {
        found
            .as_object_mut()
            .expect("object")
            .remove("styleSummaries");
    }
    value
}

#[test]
fn get_guidelines_without_kind_is_unchanged() {
    let tool = get_guidelines_snapshot();
    // A product response whose styles are all embedded guides is
    // BYTE-identical to the old one, and has no styleSummaries key at all.
    let seed = text_of(tool.call(&args(&[("query", "fintech dashboard"), ("limit", "1")])));
    assert_eq!(seed, BEFORE_SEED);
    assert!(!seed.contains("styleSummaries"));
    // With imported matches the ONLY difference is the new key, and only on
    // the matches that cite resolvable `external:` styles.
    for (pairs, before) in [
        (&[("query", "saas general"), ("limit", "3")], BEFORE_SAAS),
        (
            &[("query", "fintech banking wealth saas"), ("limit", "3")],
            BEFORE_THREE,
        ),
    ] {
        let text = text_of(tool.call(&args(pairs)));
        assert!(text.len() <= CAP, "{} bytes", text.len());
        let old: Value = serde_json::from_str(before).expect("fixture");
        assert_eq!(without_summaries(&text), old, "{pairs:?}");
        let new: Value = serde_json::from_str(&text).expect("json");
        for (found, was) in new["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .zip(old["matches"].as_array().expect("matches"))
        {
            let cites_external = was["styles"]
                .as_array()
                .expect("styles")
                .iter()
                .any(|style| style.as_str().is_some_and(|s| s.starts_with("external:")));
            let summaries = found.get("styleSummaries");
            assert_eq!(summaries.is_some(), cites_external, "{}", found["id"]);
            if let Some(summaries) = summaries {
                let summaries = summaries.as_array().expect("array");
                assert!((1..=3).contains(&summaries.len()));
                assert_eq!(
                    summaries[0]["ref"],
                    found["styles"][0]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("external:")
                );
            }
        }
    }
    // `kind` absent, empty and "product" are the same call.
    let plain = text_of(tool.call(&args(&[("query", "saas general"), ("limit", "3")])));
    for kind in ["", "product", " Product "] {
        let with_kind = text_of(tool.call(&args(&[
            ("query", "saas general"),
            ("limit", "3"),
            ("kind", kind),
        ])));
        assert_eq!(with_kind, plain, "kind {kind:?}");
    }
    // The in-app tool answers identically.
    assert_eq!(
        text_of(
            get_guidelines_in_app_snapshot()
                .call(&args(&[("query", "saas general"), ("limit", "3")]))
        ),
        plain
    );
}

#[test]
fn get_guidelines_kind_style_returns_capped_json() {
    let tool = get_guidelines_snapshot();
    let found = data(&tool, &[("kind", "style"), ("query", "dark mode oled")]);
    let matches = found["matches"].as_array().expect("matches");
    assert_eq!(matches.len(), 1, "limit defaults to 1");
    assert_eq!(matches[0]["ref"], "dark-mode-oled");
    assert_eq!(matches[0]["id"], "uiuxpm-style-dark-mode-oled");
    assert!(matches[0]["bestFor"].as_str().is_some());
    assert!(matches[0]["doNotUseFor"].as_str().is_some());
    assert_eq!(matches[0]["source"]["name"], "UI UX Pro Max (MIT)");
    assert_eq!(matches[0]["source"]["licenseSpdx"], "MIT");
    assert!(matches[0]["source"]["commit"]
        .as_str()
        .is_some_and(|c| c.len() == 40));
    assert!(found["notice"]
        .as_str()
        .expect("notice")
        .contains("not instructions"));
    assert!(found["source"].as_str().is_some());
    assert!(matches[0].get("keywords").is_none());

    // A name copied from a product match resolves, prefix and all.
    let named = data(
        &tool,
        &[
            ("kind", "style"),
            ("query", "external:minimalism-and-swiss-style"),
        ],
    );
    assert_eq!(named["matches"][0]["ref"], "minimalism-and-swiss-style");

    // limit is clamped to 3 and the text stays within 6 KB; platform is a
    // product-only filter and does not fail a style lookup.
    let many = data(
        &tool,
        &[
            ("kind", "style"),
            ("query", "glassmorphism"),
            ("limit", "99"),
            ("platform", "fridge"),
        ],
    );
    assert!((1..=3).contains(&many["matches"].as_array().expect("matches").len()));
    let none = data(
        &tool,
        &[("kind", "style"), ("query", "banana smoothie recipe")],
    );
    assert_eq!(none["matches"], serde_json::json!([]));
    assert!(none["message"]
        .as_str()
        .expect("message")
        .contains("No close style"));
    // The largest possible answer: three styles.
    for record in op_ai_skills::design_styles::style_records() {
        let text = text_of(tool.call(&args(&[
            ("kind", "style"),
            ("query", record.name.as_str()),
            ("limit", "3"),
        ])));
        assert!(text.len() <= CAP, "{}: {} bytes", record.id, text.len());
    }
    // The in-app tool serves the same data.
    let pairs = [("kind", "style"), ("query", "glassmorphism")];
    assert_eq!(
        data(&tool, &pairs),
        data(&get_guidelines_in_app_snapshot(), &pairs)
    );
    assert!(error_text(tool.call(&args(&[("kind", "style")]))).contains("query is required"));
}

#[test]
fn invalid_kind_is_a_clear_error() {
    let tool = get_guidelines_snapshot();
    for bad in ["styles", "palette", "product,style", "0"] {
        let message = error_text(tool.call(&args(&[("kind", bad), ("query", "shop")])));
        assert!(
            message.contains("kind must be one of product, style"),
            "{bad}: {message}"
        );
        assert!(message.contains(&format!("{bad:?}")), "{bad}: {message}");
    }
    // Checked before the query: a bad kind is named even without one.
    assert!(error_text(tool.call(&args(&[("kind", "x")]))).contains("kind must be"));
    // `kind` means nothing outside category=data.
    let guide = tool.call(&BTreeMap::from([
        ("category".into(), "guide".into()),
        ("topic".into(), "mobile".into()),
        ("kind".into(), "x".into()),
    ]));
    assert!(!matches!(guide, ToolOutcome::Err(..)));
}
