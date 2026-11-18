//! `get_document_info` lists stock-image credits, and only when there are any.

use std::collections::BTreeMap;

use crate::test_fixtures::{rect, state_with};
use crate::*;
use op_editor_core::EditorState;

fn call(state: &EditorState) -> ToolResponse {
    let mut registry = ToolRegistry::default();
    registry.register(Box::new(document_info_snapshot(state)));
    registry.dispatch(ToolCall {
        id: RequestId::Num(1),
        tool: "get_document_info".into(),
        arguments: BTreeMap::new(),
    })
}

fn state_with_credit() -> EditorState {
    let document = serde_json::from_value(serde_json::json!({
        "version": "1.0.0", "children": [{
            "type": "image", "id": "photo", "name": "Hero", "width": 64, "height": 64,
            "src": "https://x/y.png",
            "imageCredit": {
                "provider": "wikimedia", "creator": "Jane Doe", "license": "CC BY 4.0",
                "licenseUrl": "https://creativecommons.org/licenses/by/4.0/",
                "sourcePageUrl": "https://commons.wikimedia.org/wiki/File:Y.jpg"
            }
        }]
    }))
    .unwrap();
    EditorState::from_document(document)
}

#[test]
fn get_document_info_unchanged_without_credits() {
    let state = state_with(vec![rect("n1", "r", 0.0, 0.0, 10.0, 10.0)]);
    match call(&state) {
        ToolResponse::Ok { result, json, .. } => {
            assert!(json.is_none(), "the flat string-map reply is unchanged");
            let keys: Vec<&str> = result.keys().map(String::as_str).collect();
            assert_eq!(keys, ["active_page_index", "page_count", "total_nodes"]);
            assert_eq!(result["total_nodes"], "1");
        }
        _ => panic!("expected Ok"),
    }
}

#[test]
fn get_document_info_lists_credits_when_present() {
    match call(&state_with_credit()) {
        ToolResponse::Ok {
            json: Some(json), ..
        } => {
            let value: serde_json::Value = serde_json::from_str(&json).unwrap();
            // The existing keys keep their string values.
            assert_eq!(value["page_count"], "1");
            assert_eq!(value["total_nodes"], "1");
            let credits = value["imageCredits"].as_array().expect("imageCredits");
            assert_eq!(credits.len(), 1);
            assert_eq!(credits[0]["title"], "Hero");
            assert_eq!(credits[0]["creator"], "Jane Doe");
            assert_eq!(credits[0]["license"], "CC BY 4.0");
            assert_eq!(credits[0]["provider"], "wikimedia");
            assert_eq!(
                credits[0]["sourcePageUrl"],
                "https://commons.wikimedia.org/wiki/File:Y.jpg"
            );
        }
        _ => panic!("expected a JSON reply carrying imageCredits"),
    }
}
