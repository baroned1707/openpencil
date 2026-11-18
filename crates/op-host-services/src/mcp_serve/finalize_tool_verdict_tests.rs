//! `finalize_design` verdict: `pass` / `warn` / `fail` and the one-line `next`.
//! `complete` keeps its meaning (no blocking advisory); the verdict adds the
//! write-time scans' findings on the finalized roots.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{find, load};
use crate::mcp_serve::finalize_tool::finalize_design_snapshot;
use op_editor_core::{EditorCommand, EditorState};
use op_mcp::{McpTool, ToolOutcome};

fn state_from(children: &str) -> EditorState {
    let source = format!(r#"{{"version":"1.0","children":[{children}]}}"#);
    crate::doc_io::load_editor_state_from_source(&source, op_editor_core::Locale::EnUs)
        .expect("load fixture")
}

fn finalize(state: &EditorState) -> Value {
    let json = match finalize_design_snapshot(state).call(&BTreeMap::new()) {
        ToolOutcome::OkJson(json) | ToolOutcome::OkJsonWithCommand(json, _) => json,
        other => panic!("finalize must succeed, got {other:?}"),
    };
    serde_json::from_str(&json).expect("finalize json")
}

fn assert_one_line_next(result: &Value) {
    let next = result["next"].as_str().expect("next is a string");
    assert!(!next.trim().is_empty() && !next.contains('\n'), "{next}");
}

/// A plain (non-board) frame with one text; `fill` is the frame's fill list,
/// `ink` the text colour.
fn card(fill: &str, ink: &str) -> String {
    format!(
        r#"{{"type":"frame","id":"card","name":"Card","width":320,"height":120,"layout":"vertical",
            "fill":{fill},
            "children":[{{"type":"text","id":"t","content":"Welcome back","width":200,"height":24,
                          "fill":[{{"type":"solid","color":"{ink}"}}]}}]}}"#
    )
}

#[test]
fn a_clean_document_passes_even_with_empty_image_slots_to_fill() {
    // The shared fixture holds an empty image slot finalize materializes; open
    // slots are for enrich_images, not a reason to warn.
    let result = finalize(&load());
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["verdict"], "pass", "{result}");
    assert_one_line_next(&result);
    assert!(result.get("remainingIssues").is_none(), "{result}");
}

#[test]
fn text_over_an_image_warns_while_complete_stays_true() {
    let state = state_from(&card(
        r#"[{"type":"image","url":"https://example.test/a.jpg"}]"#,
        "#FFFFFF",
    ));
    let result = finalize(&state);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["blockingAdvisoryCount"], 0, "{result}");
    assert_eq!(result["verdict"], "warn", "{result}");
    assert_one_line_next(&result);
    assert!(
        result["next"].as_str().unwrap().contains("get_screenshot"),
        "{result}"
    );
    assert_eq!(
        result["remainingIssues"]["contrastUncertain"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{result}"
    );
}

#[test]
fn a_remaining_contrast_failure_warns_and_names_the_keys() {
    // #777777 on white is 4.48:1: below AA, yet far enough apart in luminance
    // that the finalizer leaves it alone.
    let state = state_from(&card(
        r##"[{"type":"solid","color":"#FFFFFF"}]"##,
        "#777777",
    ));
    let result = finalize(&state);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["verdict"], "warn", "{result}");
    let next = result["next"].as_str().unwrap();
    assert!(
        next.contains("contrastIssues 1") && next.contains("finalize_design again"),
        "{next}"
    );
    assert_eq!(
        result["remainingIssues"]["contrastIssues"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "{result}"
    );
    assert!(
        result["remainingIssues"].get("contrastUncertain").is_none(),
        "{result}"
    );
}

/// The finalizer must not recolour text over an image under a translucent
/// overlay: the dark text stays dark and surfaces as uncertain.
#[test]
fn the_finalizer_leaves_text_over_an_image_overlay_alone() {
    let fills = r##"[{"type":"image","url":"https://example.test/a.jpg"},{"type":"solid","color":"#000000","opacity":0.4}]"##;
    let state = state_from(&card(fills, "#111111"));
    let (json, commands) = match finalize_design_snapshot(&state).call(&BTreeMap::new()) {
        ToolOutcome::OkJson(json) => (json, Vec::new()),
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("{other:?}"),
    };
    let result: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(result["verdict"], "warn", "{result}");
    let line = result["remainingIssues"]["contrastUncertain"][0]
        .as_str()
        .expect("line");
    assert!(line.contains("node t"), "{line}");

    let mut live = state.clone();
    if !commands.is_empty() {
        assert!(live.apply(EditorCommand::Batch { commands }));
    }
    let text = find(live.active_children(), "t").expect("text survives");
    let ink = serde_json::to_value(text).unwrap();
    assert_eq!(ink["fill"][0]["color"], "#111111", "{ink}");
}

#[test]
fn the_verdict_rides_the_mcp_wire_response() {
    let state = state_from(&card(
        r#"[{"type":"image","url":"https://example.test/a.jpg"}]"#,
        "#FFFFFF",
    ));
    let mut registry = op_mcp::ToolRegistry::default();
    registry.register(Box::new(finalize_design_snapshot(&state)));
    let response = registry.dispatch(op_mcp::ToolCall {
        id: op_mcp::RequestId::Num(1),
        tool: "finalize_design".into(),
        arguments: BTreeMap::new(),
    });
    let wire: Value = serde_json::from_str(&op_mcp::tool_response_to_json(&response)).unwrap();
    let text: Value =
        serde_json::from_str(wire["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["verdict"], "warn");
    assert!(text["next"].is_string());
}
