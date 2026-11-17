//! Tests for `mcp.rs` — cross-cutting stdio dispatch + parser
//! invariants + a few read-tool registry round-trips.
//!
//! Ported off the old shell-core `Document` onto `op_editor_core::
//! EditorState`.

use crate::test_fixtures::{add_variable, state_with};
use crate::*;
use jian_ops_schema::variable::{VariableKind, VariableScalar};
use std::collections::BTreeMap;

/// Unwrap one MCP `tools/call` reply line to the inner tool-result JSON
/// text (the data rides inside the spec `content[]` envelope).
fn tool_text(reply: &str) -> String {
    let value: serde_json::Value =
        serde_json::from_str(reply.trim()).expect("json-rpc reply must parse");
    value["result"]["content"][0]["text"]
        .as_str()
        .expect("tools/call content text")
        .to_string()
}

struct EchoTool;
impl McpTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        ToolOutcome::Ok(args.clone())
    }
}

/// Deliberately badly-behaved tool: tries to invent a different
/// response id. Under the v2 trait it CAN'T — the registry stamps
/// the id.
struct LyingTool;
impl McpTool for LyingTool {
    fn name(&self) -> &str {
        "lie"
    }
    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        ToolOutcome::Ok(BTreeMap::new())
    }
}

#[test]
fn registry_starts_empty() {
    let r = ToolRegistry::default();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.names().is_empty());
}

#[test]
fn registry_dispatches_to_registered_tool() {
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    let mut args = BTreeMap::new();
    args.insert("k".into(), "v".into());
    let call = ToolCall {
        id: RequestId::Str("req-1".into()),
        tool: "echo".into(),
        arguments: args.clone(),
    };
    match r.dispatch(call) {
        ToolResponse::Ok { id, result, .. } => {
            assert_eq!(id, RequestId::Str("req-1".into()));
            assert_eq!(result.get("k"), Some(&"v".to_string()));
        }
        _ => panic!("expected Ok"),
    }
}

#[test]
fn registry_forces_id_on_response_regardless_of_tool() {
    let mut r = ToolRegistry::default();
    r.register(Box::new(LyingTool));
    let call = ToolCall {
        id: RequestId::Str("req-honest".into()),
        tool: "lie".into(),
        arguments: BTreeMap::new(),
    };
    match r.dispatch(call) {
        ToolResponse::Ok { id, .. } => {
            assert_eq!(id, RequestId::Str("req-honest".into()));
        }
        _ => panic!("expected Ok"),
    }
}

#[test]
fn response_to_json_ok_payload() {
    let mut result = BTreeMap::new();
    result.insert("k".into(), "v".into());
    let r = ToolResponse::Ok {
        id: RequestId::Num(7),
        result,
        command: None,
        json: None,
        image: None,
    };
    let j = response_to_json(&r);
    assert!(j.contains(r#""jsonrpc":"2.0""#));
    assert!(j.contains(r#""id":7"#));
    assert!(j.contains(r#""result":"#));
    assert!(j.contains(r#""k":"v""#));
}

#[test]
fn response_to_json_err_payload() {
    let r = ToolResponse::Err {
        id: RequestId::Str("req".into()),
        code: ToolErrorCode::UnknownTool,
        message: "no such tool".into(),
    };
    let j = response_to_json(&r);
    assert!(j.contains(r#""id":"req""#));
    assert!(j.contains(r#""code":-32601"#));
    assert!(j.contains(r#""message":"no such tool""#));
}

#[test]
fn tool_response_to_json_wraps_ok_in_mcp_content_envelope() {
    // MCP-spec `tools/call` result (TS parity): the data rides inside
    // `result.content[].text` as a JSON string, no `isError`.
    let mut result = BTreeMap::new();
    result.insert("ok".into(), "true".into());
    let r = ToolResponse::Ok {
        id: RequestId::Num(7),
        result,
        command: None,
        json: None,
        image: None,
    };
    let j = tool_response_to_json(&r);
    assert!(j.contains(r#""id":7"#), "{j}");
    assert!(j.contains(r#""content":[{"type":"text","text":"#), "{j}");
    assert!(!j.contains("isError"), "{j}");
    assert_eq!(tool_text(&j), r#"{"ok":"true"}"#);
}

#[test]
fn ok_json_rides_verbatim_in_both_serializers() {
    // A nested-JSON read result (ToolOutcome::OkJson) must serialize as
    // arbitrary nested JSON — NOT the flat string-map encoding — so it
    // matches TS pen-mcp's `JSON.stringify(result)` shapes exactly.
    let nested = r#"{"layout":[{"id":"a","x":0,"y":0}]}"#;
    let r = ToolResponse::Ok {
        id: RequestId::Num(9),
        result: BTreeMap::new(),
        command: None,
        json: Some(nested.to_string()),
        image: None,
    };
    // tools/call envelope: text block holds the nested JSON verbatim.
    assert_eq!(tool_text(&tool_response_to_json(&r)), nested);
    // direct JSON-RPC: result IS the nested JSON object (array preserved).
    let direct = response_to_json(&r);
    assert!(
        direct.contains(r#""result":{"layout":[{"id":"a","x":0,"y":0}]}"#),
        "{direct}"
    );
}

#[test]
fn tool_response_to_json_emits_image_content_block_when_image_is_some() {
    // When `image` is `Some`, the serializer emits an ImageContent block
    // BEFORE the text block so vision-capable MCP clients receive the
    // image as multimodal content.
    let r = ToolResponse::Ok {
        id: RequestId::Num(3),
        result: BTreeMap::new(),
        command: None,
        json: Some(r#"{"nodeId":"n1","format":"png"}"#.into()),
        image: Some(Box::new(ImageContent {
            data: "iVBORw0KGgo=".into(),
            mime_type: "image/png".into(),
        })),
    };
    let j = tool_response_to_json(&r);
    // Parse the JSON-RPC envelope to verify structure.
    let v: serde_json::Value = serde_json::from_str(&j).expect("valid JSON-RPC");
    let content = v["result"]["content"].as_array().expect("content array");
    assert_eq!(content.len(), 2, "expected image + text blocks");
    // First block must be the image.
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["data"], "iVBORw0KGgo=");
    assert_eq!(content[0]["mimeType"], "image/png");
    // Second block must be text with the metadata.
    assert_eq!(content[1]["type"], "text");
    let text_val = content[1]["text"].as_str().expect("text string");
    let meta: serde_json::Value = serde_json::from_str(text_val).expect("metadata parses as JSON");
    assert_eq!(meta["nodeId"], "n1");
    assert_eq!(meta["format"], "png");
    // No isError for successful results.
    assert!(v["result"].get("isError").is_none(), "{j}");
}

#[test]
fn tool_response_to_json_image_without_text_metadata_still_emits_blocks() {
    // When image is set but json/result are empty, the text block is still
    // present (with empty object) — MCP clients expect at least one text
    // block for tool result reporting.
    let r = ToolResponse::Ok {
        id: RequestId::Num(4),
        result: BTreeMap::new(),
        command: None,
        json: None,
        image: Some(Box::new(ImageContent {
            data: "AAAA".into(),
            mime_type: "image/png".into(),
        })),
    };
    let j = tool_response_to_json(&r);
    assert!(j.contains(r#""type":"image""#), "{j}");
    assert!(j.contains(r#""type":"text""#), "{j}");
    assert!(j.contains(r#""data":"AAAA""#), "{j}");
}

#[test]
fn tool_response_to_json_marks_tool_error_with_iserror() {
    // A tool-level failure is an `isError` result, NOT a JSON-RPC error.
    let r = ToolResponse::Err {
        id: RequestId::Num(9),
        code: ToolErrorCode::ToolFailed,
        message: "boom".into(),
    };
    let j = tool_response_to_json(&r);
    assert!(j.contains(r#""id":9"#), "{j}");
    assert!(j.contains(r#""isError":true"#), "{j}");
    assert!(
        !j.contains(r#""error":"#),
        "tool error must not be JSON-RPC error: {j}"
    );
    assert_eq!(tool_text(&j), "Error: [ToolFailed] boom", "{j}");
}

/// Invariant: every tool error leads its text with the `ToolErrorCode` name,
/// so a client can branch on the failure kind, while `isError` stays true.
#[test]
fn tool_error_text_leads_with_the_error_code_name() {
    for (code, name) in [
        (ToolErrorCode::MissingArgument, "MissingArgument"),
        (ToolErrorCode::InvalidArgument, "InvalidArgument"),
        (ToolErrorCode::ToolFailed, "ToolFailed"),
        (ToolErrorCode::UnknownTool, "UnknownTool"),
        (ToolErrorCode::Internal, "Internal"),
    ] {
        let reply = tool_response_to_json(&ToolResponse::Err {
            id: RequestId::Num(1),
            code,
            message: "nope".into(),
        });
        assert_eq!(tool_text(&reply), format!("Error: [{name}] nope"));
        assert!(reply.contains(r#""isError":true"#), "{reply}");
    }
}

/// Invariant: the PNG next to an `image` block is not repeated in the text
/// block, and a result with no image block keeps every field verbatim.
#[test]
fn inline_image_is_dropped_from_the_text_block_only_when_an_image_block_carries_it() {
    let ok = |json: &str, image: Option<Box<ImageContent>>| ToolResponse::Ok {
        id: RequestId::Num(1),
        result: Default::default(),
        command: None,
        json: Some(json.to_string()),
        image,
    };
    let png = || {
        Some(Box::new(ImageContent {
            data: "AAAA".into(),
            mime_type: "image/png".into(),
        }))
    };
    let with_image = tool_response_to_json(&ok(r#"{"format":"png","image_base64":"AAAA"}"#, png()));
    let value: serde_json::Value = serde_json::from_str(&with_image).unwrap();
    assert_eq!(value["result"]["content"][0]["data"], "AAAA");
    assert_eq!(
        value["result"]["content"][1]["text"], r#"{"format":"png"}"#,
        "{with_image}"
    );
    assert_eq!(with_image.matches("AAAA").count(), 1, "{with_image}");

    // No image block: the field is data the caller asked for, so it stays.
    let no_image = tool_response_to_json(&ok(r#"{"format":"png","image_base64":"AAAA"}"#, None));
    assert!(tool_text(&no_image).contains("image_base64"), "{no_image}");
    // An image block with metadata that has no such field, or is not an
    // object, passes the metadata through untouched.
    let plain = tool_response_to_json(&ok(r#"{"format":"png"}"#, png()));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&plain).unwrap()["result"]["content"][1]["text"],
        r#"{"format":"png"}"#
    );
    let array = tool_response_to_json(&ok(r#"["image_base64"]"#, png()));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&array).unwrap()["result"]["content"][1]["text"],
        r#"["image_base64"]"#
    );
}

#[test]
fn parse_tool_call_round_trips_through_registry() {
    let line = r#"{"jsonrpc":"2.0","id":42,"method":"echo","params":{}}"#;
    let call = parse_tool_call(line).expect("parse");
    assert_eq!(call.id, RequestId::Num(42));
    assert_eq!(call.tool, "echo");
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    match r.dispatch(call) {
        ToolResponse::Ok { id, .. } => assert_eq!(id, RequestId::Num(42)),
        _ => panic!(),
    }
}

#[test]
fn run_stdio_dispatches_multi_line_stream() {
    use std::io::{BufReader, Cursor};
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":{}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"echo\",\"params\":{}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":\"x\",\"method\":\"unknown\",\"params\":{}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio(&r, &mut reader, &mut writer).unwrap();
    let out = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains(r#""id":1"#));
    assert!(lines[1].contains(r#""id":2"#));
    assert!(lines[2].contains(r#""id":"x""#));
    // Unknown tool is reported as an MCP `isError` result (TS parity:
    // server.ts wraps every handler error in the content envelope), not a
    // JSON-RPC `error`.
    assert!(lines[2].contains(r#""isError":true"#), "{}", lines[2]);
}

#[test]
fn run_stdio_skips_malformed_lines() {
    use std::io::{BufReader, Cursor};
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    let input = b"garbage\n\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"echo\",\"params\":{}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio(&r, &mut reader, &mut writer).unwrap();
    let out = String::from_utf8(writer).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains(r#""id":7"#));
}

#[test]
fn run_stdio_emits_error_when_parse_fails_so_clients_dont_hang() {
    use std::io::{BufReader, Cursor};
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    let bad = br#"{"jsonrpc":"2.0","id":42,"method":"replace_node","params":{"node_id":"1","drop_children":{}}}"#;
    let mut input: Vec<u8> = bad.to_vec();
    input.push(b'\n');
    let mut reader = BufReader::new(Cursor::new(input));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio(&r, &mut reader, &mut writer).unwrap();
    let out = String::from_utf8(writer).unwrap();
    assert!(
        out.contains(r#""id":42"#),
        "response must echo the request id: {out}"
    );
    assert!(
        out.contains(r#""error""#),
        "response must be a typed error: {out}"
    );
    assert!(
        out.contains("malformed tool call"),
        "error message names the cause: {out}"
    );
    // ... and names the tool, the offending key, the fix and what the tool
    // does accept, so the agent can correct the very next call.
    for needle in [
        "'replace_node'",
        "'drop_children'",
        "JSON-encoded string",
        "accepts arrays/objects only for: data",
    ] {
        assert!(out.contains(needle), "missing {needle}: {out}");
    }
}

/// Invariant: the parse-failure text names the tool and the key, whichever
/// way the call is malformed, and always keeps the `malformed tool call` lead.
#[test]
fn explain_unparseable_call_names_the_tool_and_the_offending_key() {
    use super::explain_unparseable_call as explain;
    let mcp = |tool: &str, arguments: &str| {
        format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
        )
    };
    let nested = explain(&mcp("get_node", r#"{"node_id":["a"],"depth":{"x":1}}"#));
    assert!(
        nested.starts_with("malformed tool call for 'get_node'"),
        "{nested}"
    );
    assert!(nested.contains("'node_id', 'depth'") || nested.contains("'depth', 'node_id'"));
    assert!(
        nested.contains("'get_node' takes no array or object arguments"),
        "{nested}"
    );

    let some = explain(&mcp("read_nodes", r#"{"depth":[1]}"#));
    assert!(
        some.contains("'depth'") && some.contains("only for: nodeIds"),
        "{some}"
    );

    let add = explain(&mcp("add_widget", r#"{"depth":"1","x":{"y":1}}"#));
    assert!(
        add.contains("malformed tool call for 'add_widget'"),
        "{add}"
    );
    assert!(explain(&mcp("get_node", r#""oops""#)).contains("`arguments` must be a JSON object"));
    assert!(explain("{not json").starts_with("malformed tool call:"));
    assert!(explain(r#"{"id":1,"method":"tools/call","params":{}}"#).contains("params.name"));
    let truncated = explain(&mcp("get_node", r#"{"node_id":"a"}"#));
    assert!(truncated.contains("could not be parsed"), "{truncated}");
}

#[test]
fn run_stdio_skips_lines_without_an_id() {
    use std::io::{BufReader, Cursor};
    let mut r = ToolRegistry::default();
    r.register(Box::new(EchoTool));
    let input = b"garbage\n{\"method\":\"x\",\"params\":{}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio(&r, &mut reader, &mut writer).unwrap();
    let out = String::from_utf8(writer).unwrap();
    assert!(
        out.is_empty(),
        "id-less lines must be dropped silently: {out:?}"
    );
}

#[test]
fn run_stdio_demotes_write_tool_response_to_error_without_applier() {
    // The read-only `run_stdio` path demotes any `OkWithCommand`
    // response to `Internal` so a misleading "wrote: true" can't
    // reach the client.
    use std::io::{BufReader, Cursor};
    let mut s = state_with(vec![]);
    add_variable(
        &mut s,
        "brand",
        VariableKind::Color,
        VariableScalar::Str("#ff8800".into()),
    );
    let mut r = ToolRegistry::default();
    r.register(Box::new(set_variable_color_snapshot(&s)));
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"set_variable_color\",\"params\":{\"name\":\"brand\",\"hex\":\"#00ff00\"}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio(&r, &mut reader, &mut writer).unwrap();
    let out = String::from_utf8(writer).unwrap();
    assert!(
        out.contains(r#""isError":true"#),
        "read-only run_stdio must demote write OkWithCommand to an isError result; got {out}"
    );
    assert!(out.contains("host rejected command"));
}

#[test]
fn run_stdio_with_applier_applies_write_command_then_writes_success() {
    use std::io::{BufReader, Cursor};
    let mut s = state_with(vec![]);
    add_variable(
        &mut s,
        "brand",
        VariableKind::Color,
        VariableScalar::Str("#ff8800".into()),
    );
    let mut r = ToolRegistry::default();
    r.register(Box::new(set_variable_color_snapshot(&s)));
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"set_variable_color\",\"params\":{\"name\":\"brand\",\"hex\":\"#00ff00\"}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    let mut applied: Vec<EditorCommand> = Vec::new();
    let mut applied_tool_names: Vec<String> = Vec::new();
    run_stdio_with_applier(&r, &mut reader, &mut writer, |tool_name, cmd| {
        applied_tool_names.push(tool_name.to_string());
        applied.push(cmd.clone());
        true
    })
    .unwrap();
    assert_eq!(applied_tool_names, vec!["set_variable_color"]);
    assert_eq!(applied.len(), 1);
    assert!(matches!(
        applied[0],
        EditorCommand::SetVariableColor { ref name, ref hex }
        if name == "brand" && hex == "#00ff00"
    ));
    let out = String::from_utf8(writer).unwrap();
    assert!(out.contains(r#""id":1"#));
    assert!(tool_text(&out).contains(r#""wrote":"true""#), "{out}");
    assert!(!out.contains(r#""isError""#), "no error flag: {out}");
}

#[test]
fn run_stdio_with_applier_demotes_when_applier_rejects() {
    use std::io::{BufReader, Cursor};
    let mut s = state_with(vec![]);
    add_variable(
        &mut s,
        "brand",
        VariableKind::Color,
        VariableScalar::Str("#ff8800".into()),
    );
    let mut r = ToolRegistry::default();
    r.register(Box::new(set_variable_color_snapshot(&s)));
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"set_variable_color\",\"params\":{\"name\":\"brand\",\"hex\":\"#00ff00\"}}\n";
    let mut reader = BufReader::new(Cursor::new(input.as_ref()));
    let mut writer: Vec<u8> = Vec::new();
    run_stdio_with_applier(&r, &mut reader, &mut writer, |_, _| false).unwrap();
    let out = String::from_utf8(writer).unwrap();
    assert!(out.contains(r#""isError":true"#), "{out}");
    assert!(out.contains("host rejected"));
}

#[test]
fn json_escape_handles_special_chars() {
    let r = ToolResponse::Err {
        id: RequestId::Str("x\"y".into()),
        code: ToolErrorCode::Internal,
        message: "line1\nline2".into(),
    };
    let j = response_to_json(&r);
    assert!(j.contains(r#""x\"y""#));
    assert!(j.contains(r#""line1\nline2""#));
}
