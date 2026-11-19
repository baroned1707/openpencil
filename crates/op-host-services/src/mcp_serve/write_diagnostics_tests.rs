//! Tests for the design feedback appended to external MCP writes.

use op_editor_core::EditorState;
use op_mcp::RequestId;
use serde_json::{json, Map, Value};

use super::super::tool_profile::McpAccessProfile;
use super::super::{process_message_with_applier_profiled, tool_text, TOOL_SCHEMAS};
use super::{append_feedback, wants_feedback};

/// A card that clips an absolutely placed button: a defect the layout scan
/// reports (`clipped-control`), plus one misspelled icon.
const DEFECTIVE_SCRIPT: &str = r##"
I(null, {"type":"frame","id":"card","name":"Card","width":294,"height":236,
  "layout":"none","clipContent":true,"fill":[{"type":"solid","color":"#FFFFFF"}],
  "children":[
    {"type":"frame","id":"add","role":"button","x":282,"y":176,"width":44,"height":44},
    {"type":"icon_font","id":"star","iconFontName":"no-such-glyph-xyz","width":20,"height":20}
  ]});
"##;

const CLEAN_SCRIPT: &str = r##"
I(null, {"type":"frame","id":"card","name":"Card","width":294,"height":236,
  "layout":"none","fill":[{"type":"solid","color":"#FFFFFF"}],
  "children":[
    {"type":"icon_font","id":"star","iconFontName":"search","width":20,"height":20}
  ]});
"##;

fn line(tool: &str, arguments: Value) -> String {
    json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    })
    .to_string()
}

/// Run `line` the way both MCP transports do: through the applier seam, applying
/// the command to `state`.
fn run(state: &mut EditorState, line: &str) -> String {
    process_message_with_applier_profiled(
        state,
        line,
        McpAccessProfile::UNRESTRICTED,
        |_, state, cmd| state.apply(cmd.clone()),
    )
    .expect("dispatch")
    .expect("response")
}

/// The same call through the pre-feature path: the registry and applier only.
fn run_without_feedback(state: &mut EditorState, line: &str) -> String {
    let registry = super::super::rebuild_registry(
        state,
        Some(op_mcp::parse_tool_call(line).expect("call").tool.as_str()),
        McpAccessProfile::UNRESTRICTED,
    );
    super::super::process_tool_message_with_registry(&registry, line, |_, cmd| {
        state.apply(cmd.clone())
    })
    .expect("dispatch")
    .expect("response")
}

fn result_json(response: &str) -> Value {
    serde_json::from_str(&tool_text(response)).expect("tool result JSON")
}

/// Invariant: a script that builds a card clipping its own button yields
/// `layoutIssues` (and `iconIssues` for the unknown glyph) in the result of
/// an external `batch_design`, with at most 12 short lines per key.
#[test]
fn external_batch_design_result_carries_layout_issues() {
    let mut state = EditorState::new();
    let response = run(
        &mut state,
        &line("batch_design", json!({ "script": DEFECTIVE_SCRIPT })),
    );
    assert!(!response.contains(r#""isError":true"#), "{response}");
    let result = result_json(&response);
    let layout = result["layoutIssues"].as_array().expect("layoutIssues key");
    assert!(
        layout.iter().any(|issue| issue
            .as_str()
            .is_some_and(|s| s.contains("clipped-control"))),
        "{layout:?}"
    );
    let icons = result["iconIssues"].as_array().expect("iconIssues key");
    assert!(icons[0]
        .as_str()
        .expect("line")
        .contains("'no-such-glyph-xyz' is not a lucide glyph"));
    for key in ["layoutIssues", "iconIssues"] {
        assert!(result[key].as_array().expect(key).len() <= 12);
    }
    assert!(
        tool_text(&response).len() < 4096 + 2048,
        "appendix stays small"
    );
}

/// Invariant: a write that leaves nothing to report returns exactly the bytes
/// the pre-feature path returns, so existing clients and tests are unaffected.
#[test]
fn clean_write_result_is_unchanged() {
    let call = line("batch_design", json!({ "script": CLEAN_SCRIPT }));
    let with_feedback = run(&mut EditorState::new(), &call);
    let baseline = run_without_feedback(&mut EditorState::new(), &call);
    assert_eq!(with_feedback, baseline);
    assert!(!with_feedback.contains("Issues"), "{with_feedback}");
}

/// Invariant: a read tool, a rejected write and an error result never get the
/// appendix.
#[test]
fn only_applied_writes_of_the_six_tools_get_feedback() {
    for tool in [
        "batch_design",
        "design_content",
        "design_refine",
        "insert_node",
        "replace_node",
        "update_node",
    ] {
        assert!(wants_feedback(tool), "{tool}");
    }
    for tool in [
        "snapshot_layout",
        "get_design_quality",
        "finalize_design",
        "set_node_fill_hex",
    ] {
        assert!(!wants_feedback(tool), "{tool}");
    }
    // The applier refusing the command (nothing was written) adds nothing.
    let mut state = EditorState::new();
    let call = line("batch_design", json!({ "script": DEFECTIVE_SCRIPT }));
    let refused = process_message_with_applier_profiled(
        &mut state,
        &call,
        McpAccessProfile::UNRESTRICTED,
        |_, _, _| false,
    )
    .expect("dispatch")
    .expect("response");
    assert!(!refused.contains("layoutIssues"), "{refused}");
}

/// Invariant: a panic or an error inside the diagnostics leaves the write
/// result exactly as the tool produced it.
#[test]
fn diagnostics_never_fail_the_write() {
    let id = RequestId::Num(7);
    let ok = tool_ok(r#"{"wrote":"true"}"#);
    let panicked = append_feedback(ok.clone(), &id, || panic!("scan blew up"));
    assert_eq!(panicked, ok);
    let failed = append_feedback(ok.clone(), &id, || Err("page is gone".to_string()));
    assert_eq!(failed, ok);
    let empty = append_feedback(ok.clone(), &id, || Ok(Map::new()));
    assert_eq!(empty, ok);
    let mut fields = Map::new();
    fields.insert("layoutIssues".into(), json!(["frame f: collapsed"]));
    let merged = append_feedback(ok.clone(), &id, || Ok(fields));
    assert_eq!(
        serde_json::from_str::<Value>(&tool_text(&merged)).expect("merged"),
        json!({ "wrote": "true", "layoutIssues": ["frame f: collapsed"] })
    );
    // An error result is never decorated, and the diagnostics never run.
    let error = op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Err {
        id: id.clone(),
        code: op_mcp::ToolErrorCode::ToolFailed,
        message: "nope".into(),
    });
    assert_eq!(
        append_feedback(error.clone(), &id, || panic!("must not run")),
        error
    );
}

fn tool_ok(json_text: &str) -> String {
    op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Ok {
        id: RequestId::Num(7),
        result: Default::default(),
        command: None,
        json: Some(json_text.to_string()),
        image: None,
    })
}

/// Invariant: the in-app wrapper and the MCP write path report the same
/// findings, because both call `post_write_diagnostics`.
#[test]
fn in_app_wrapper_and_mcp_share_the_same_diagnostics_function() {
    let args = json!({ "script": DEFECTIVE_SCRIPT }).to_string();
    let mut in_app_state = EditorState::new();
    let (in_app, mutated) = crate::design_agent_tools::execute_design_tool_with_root_seed_guard(
        &mut in_app_state,
        "batch_design",
        &args,
        None,
        None,
    );
    assert!(mutated && !in_app.is_error, "{}", in_app.content);
    let in_app: Value = serde_json::from_str(&in_app.content).expect("in-app envelope");

    let mut mcp_state = EditorState::new();
    let mcp = result_json(&run(
        &mut mcp_state,
        &line("batch_design", json!({ "script": DEFECTIVE_SCRIPT })),
    ));
    for key in ["layoutIssues", "iconIssues"] {
        assert!(in_app[key].is_array(), "in-app lacks {key}: {in_app}");
        assert_eq!(
            in_app[key], mcp[key],
            "{key} differs between in-app and MCP"
        );
    }
    assert!(in_app["layoutHint"].is_string() && mcp["layoutHint"].is_string());
}

/// Invariant: a small writer (`update_node`) carries the feedback too: renaming
/// a glyph to a misspelling reports it in that call's result.
#[test]
fn small_writer_update_node_carries_feedback() {
    let mut state = EditorState::new();
    run(
        &mut state,
        &line("batch_design", json!({ "script": CLEAN_SCRIPT })),
    );
    let icon_id = {
        use op_editor_core::pen_node_ext::PenNodeExt;
        let card = &state.active_children()[0];
        card.children().expect("card children")[0]
            .id_str()
            .to_string()
    };
    let response = run(
        &mut state,
        &line(
            "update_node",
            json!({ "nodeId": icon_id, "data": "{\"iconFontName\":\"no-such-glyph-xyz\"}" }),
        ),
    );
    assert!(!response.contains(r#""isError":true"#), "{response}");
    let result = result_json(&response);
    assert!(
        result["iconIssues"][0]
            .as_str()
            .expect("icon line")
            .contains("'no-such-glyph-xyz'"),
        "{result}"
    );
}

/// Invariant: adding the feedback changed neither the static catalog size nor
/// the six-tool lean set, and the new description sentences keep the lint's
/// rules (no porting jargon) and the pinned phrases.
#[test]
fn tool_count_and_lean_set_unchanged() {
    assert_eq!(TOOL_SCHEMAS.len(), 144);
    assert_eq!(super::super::tool_catalog::LEAN_TOOLS.len(), 6);
}

#[test]
fn description_lint_still_passes() {
    let describe = |name: &str| -> String {
        let entry = TOOL_SCHEMAS
            .iter()
            .find(|schema| schema.contains(&format!("\"name\":\"{name}\"")))
            .expect("schema");
        serde_json::from_str::<Value>(entry).expect("schema JSON")["description"]
            .as_str()
            .expect("description")
            .to_string()
    };
    let batch = describe("batch_design");
    assert!(batch.contains("generate every new UI tree with script"));
    assert!(batch.contains("operations is ONLY the edit/refine surface"));
    assert!(batch.contains("layoutIssues") && batch.contains("before finalize_design"));
    let layout = describe("snapshot_layout");
    assert!(
        layout.contains("clipped")
            && layout.contains("collapsed")
            && layout.contains("document px")
    );
    for text in [batch, layout] {
        assert!(!text.trim_start().starts_with("TS"));
        assert!(!text.to_lowercase().contains("ts compat"));
    }
}

/// Invariant: `update_node` skips the feedback on a page above the node limit
/// (it would pay the clone + repair on every small edit), while the whole-tree
/// writers keep it at any size and a small page still gets it.
#[test]
fn update_node_skips_feedback_only_on_a_large_page() {
    use super::{too_costly_for_small_writer_for_tests, SMALL_WRITER_NODE_LIMIT};
    let mut state = EditorState::new();
    run(
        &mut state,
        &line("batch_design", json!({ "script": CLEAN_SCRIPT })),
    );
    assert!(!too_costly_for_small_writer_for_tests(
        "update_node",
        &state
    ));
    let rows = SMALL_WRITER_NODE_LIMIT + 5;
    let script = format!(
        "const root=I(\"document\",{{type:\"frame\",name:\"Big\",width:400,height:400}});\nfor (let i=0;i<{rows};i++) I(root,{{type:\"rectangle\",name:\"r\"+i,width:4,height:4}});"
    );
    run(
        &mut state,
        &line("batch_design", json!({ "script": script })),
    );
    assert!(too_costly_for_small_writer_for_tests("update_node", &state));
    assert!(!too_costly_for_small_writer_for_tests(
        "batch_design",
        &state
    ));
}

/// A page with one text node in `family` (a defect-free card otherwise).
fn text_script(family: &str) -> String {
    format!(
        r##"I(null, {{"type":"frame","id":"card","name":"Card","width":294,"height":236,
  "layout":"none","fill":[{{"type":"solid","color":"#FFFFFF"}}],
  "children":[{{"type":"text","id":"label","content":"Hello","fontFamily":"{family}",
    "fontSize":16,"x":10,"y":10,"width":120,"height":24}}]}});"##
    )
}

/// Invariant: an unresolvable `fontFamily` is reported under `fontIssues`,
/// with the same line, by the in-app wrapper and by the MCP write path (both
/// call `post_write_diagnostics`). No catalog file is involved: the lookup is
/// pointed at "no file" so the test never reads the developer's config root.
#[test]
fn font_issues_ride_the_shared_diagnostics_in_app_and_mcp() {
    let script = text_script("Zzyzx Display");
    let args = json!({ "script": script }).to_string();
    op_chat_agent::design_agent_tools::with_font_catalog_file(None, || {
        let mut in_app_state = EditorState::new();
        in_app_state.editor_ui.system_fonts_loaded = true;
        let (in_app, mutated) = crate::design_agent_tools::execute_design_tool_with_root_seed_guard(
            &mut in_app_state,
            "batch_design",
            &args,
            None,
            None,
        );
        assert!(mutated && !in_app.is_error, "{}", in_app.content);
        let in_app: Value = serde_json::from_str(&in_app.content).expect("in-app envelope");

        let mut mcp_state = EditorState::new();
        mcp_state.editor_ui.system_fonts_loaded = true;
        let mcp = result_json(&run(
            &mut mcp_state,
            &line("batch_design", json!({ "script": script })),
        ));
        let lines = mcp["fontIssues"].as_array().expect("MCP fontIssues");
        assert_eq!(lines.len(), 1, "{mcp}");
        assert!(lines[0]
            .as_str()
            .expect("line")
            .starts_with("font 'Zzyzx Display': not installed"));
        assert_eq!(in_app["fontIssues"], mcp["fontIssues"]);

        // A headless process (system fonts unknown, no catalog) cannot judge a
        // name, so it says nothing rather than guessing.
        let mut headless = EditorState::new();
        let quiet = result_json(&run(
            &mut headless,
            &line("batch_design", json!({ "script": script })),
        ));
        assert!(quiet.get("fontIssues").is_none(), "{quiet}");
    });
}

/// Invariant: a write whose fonts all resolve (a bundled family) returns the
/// exact bytes of the pre-feature path.
#[test]
fn clean_write_result_is_still_byte_identical() {
    let call = line("batch_design", json!({ "script": text_script("Inter") }));
    op_chat_agent::design_agent_tools::with_font_catalog_file(None, || {
        let mut with = EditorState::new();
        with.editor_ui.system_fonts_loaded = true;
        let mut without = EditorState::new();
        without.editor_ui.system_fonts_loaded = true;
        let with_feedback = run(&mut with, &call);
        let baseline = run_without_feedback(&mut without, &call);
        assert_eq!(with_feedback, baseline);
        assert!(!with_feedback.contains("fontIssues"), "{with_feedback}");
    });
}
