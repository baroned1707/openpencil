//! A misspelled glyph name in an external write is corrected on the document
//! and reported as `renamed: from -> to` under `iconIssues`, on both MCP
//! transports. The headless transport repairs the document inside its applier;
//! the live transport applies to a local snapshot and the editor repairs its
//! real document separately. Each test checks the DOCUMENT and the RESULT.

use op_chat_agent::design_agent_tools::IconNameSnapshot;
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::{EditorCommand, EditorState};
use serde_json::{json, Value};

use super::super::tool_profile::McpAccessProfile;
use super::super::{process_message, process_message_with_applier_profiled, tool_text};

fn script(icon_name: &str) -> String {
    format!(
        r##"I(null, {{"type":"frame","id":"card","name":"Card","width":200,"height":100,
  "layout":"none","fill":[{{"type":"solid","color":"#FFFFFF"}}],
  "children":[{{"type":"icon_font","id":"star","iconFontName":"{icon_name}",
                "iconFontFamily":"lucide","width":20,"height":20}}]}});"##
    )
}

fn batch_design_line(icon_name: &str) -> String {
    json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "batch_design", "arguments": { "script": script(icon_name) } },
    })
    .to_string()
}

fn icon_name_on_page(state: &EditorState) -> (String, Option<String>) {
    let card = &state.active_children()[0];
    match &card.children().expect("card children")[0] {
        jian_ops_schema::node::PenNode::IconFont(icon) => {
            (icon.icon_font_name.clone(), icon.icon_font_family.clone())
        }
        other => panic!("expected the icon, got {other:?}"),
    }
}

fn icon_issues(response: &str) -> Vec<String> {
    let result: Value = serde_json::from_str(&tool_text(response)).expect("result json");
    result["iconIssues"]
        .as_array()
        .map(|lines| {
            lines
                .iter()
                .map(|line| line.as_str().expect("line").to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn headless_write_renames_the_document_saves_it_and_reports_the_rename() {
    let dir = std::env::temp_dir().join(format!("op-icon-rename-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("doc.op");
    let mut state = EditorState::new();
    super::super::save_editor_state(&state, &path).expect("seed file");

    let response = process_message(&mut state, &path, &batch_design_line("serch"))
        .expect("dispatch")
        .expect("response");
    let lines = icon_issues(&response);
    assert_eq!(lines.len(), 1, "{lines:?}");
    // The batch assigns its own node ids, so only the shape of the line is fixed.
    assert!(
        lines[0].starts_with("renamed: serch -> search (icon "),
        "{lines:?}"
    );
    // The family is exactly as written; only the name moved.
    assert_eq!(
        icon_name_on_page(&state),
        ("search".into(), Some("lucide".into()))
    );
    let saved = super::super::load_or_initialize_editor_state(&path).expect("reload");
    assert_eq!(
        icon_name_on_page(&saved).0,
        "search",
        "the saved file holds the fix"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn live_write_reports_the_rename_and_the_editors_own_repair_makes_it_on_the_real_document() {
    // The live applier updates a local snapshot only, and hands the command to
    // the editor, which applies it and runs the same repairs on its document.
    let mut snapshot = EditorState::new();
    let mut real = EditorState::new();
    let mut handed_over: Option<EditorCommand> = None;
    let response = process_message_with_applier_profiled(
        &mut snapshot,
        &batch_design_line("heartt"),
        McpAccessProfile::UNRESTRICTED,
        |_, local, command| {
            handed_over = Some(command.clone());
            local.apply(command.clone())
        },
    )
    .expect("dispatch")
    .expect("response");
    let lines = icon_issues(&response);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("renamed: heartt -> heart (icon "),
        "{lines:?}"
    );

    let command = handed_over.expect("the write reached the applier");
    let before = IconNameSnapshot::capture(&real);
    assert!(real.apply(command));
    super::super::run_write_repairs_after_apply(&mut real, &before);
    assert_eq!(icon_name_on_page(&real).0, "heart");
}

#[test]
fn an_unresolvable_name_is_kept_and_reported_as_before() {
    let mut state = EditorState::new();
    let response = process_message_with_applier_profiled(
        &mut state,
        &batch_design_line("no-such-glyph-xyz"),
        McpAccessProfile::UNRESTRICTED,
        |_, local, command| local.apply(command.clone()),
    )
    .expect("dispatch")
    .expect("response");
    let lines = icon_issues(&response);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("'no-such-glyph-xyz' is not a lucide glyph"));
    assert_eq!(icon_name_on_page(&state).0, "no-such-glyph-xyz");
}

#[test]
fn names_that_already_paint_leave_the_result_byte_identical_to_the_plain_path() {
    for name in ["check-circle", "cart", "home", "search"] {
        let call = batch_design_line(name);
        let with_feedback = process_message_with_applier_profiled(
            &mut EditorState::new(),
            &call,
            McpAccessProfile::UNRESTRICTED,
            |_, local, command| local.apply(command.clone()),
        )
        .expect("dispatch")
        .expect("response");

        let mut plain_state = EditorState::new();
        let registry = super::super::rebuild_registry(
            &plain_state,
            Some("batch_design"),
            McpAccessProfile::UNRESTRICTED,
        );
        let plain = super::super::process_tool_message_with_registry(&registry, &call, |_, cmd| {
            plain_state.apply(cmd.clone())
        })
        .expect("dispatch")
        .expect("response");
        assert_eq!(with_feedback, plain, "{name}");
        assert!(!with_feedback.contains("iconIssues"), "{name}");
    }
}
