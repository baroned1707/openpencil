//! The `import_svg` result: byte-identical when nothing is skipped, with a
//! compact `skipped` list when the SVG uses features the importer drops.

use super::write_tools::import_svg_snapshot;
use super::{EditorCommand, McpTool, ToolOutcome};
use std::collections::BTreeMap;

fn call(svg: &str) -> ToolOutcome {
    let mut args = BTreeMap::new();
    args.insert("svg".to_string(), svg.to_string());
    import_svg_snapshot().call(&args)
}

#[test]
fn a_clean_svg_keeps_the_flat_wrote_result() {
    match call(r#"<svg><rect width="10" height="10"/></svg>"#) {
        ToolOutcome::OkWithCommand(result, EditorCommand::ImportSvg { .. }) => {
            assert_eq!(result.len(), 1);
            assert_eq!(result.get("wrote").map(String::as_str), Some("true"));
        }
        other => panic!("expected the plain result, got {other:?}"),
    }
}

#[test]
fn skipped_features_ride_next_to_wrote() {
    let svg = r#"<svg><text>hi</text><text>there</text><rect width="9" height="9" transform="scale(2)"/></svg>"#;
    match call(svg) {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::ImportSvg { svg: sent, .. }) => {
            assert_eq!(sent, svg, "the command still carries the whole SVG");
            let value: serde_json::Value = serde_json::from_str(&json).expect("json");
            assert_eq!(value["wrote"], "true");
            assert_eq!(
                value["skipped"],
                serde_json::json!([
                    {"feature": "text", "count": 2},
                    {"feature": "transform", "count": 1}
                ])
            );
        }
        other => panic!("expected a result with skipped, got {other:?}"),
    }
}
