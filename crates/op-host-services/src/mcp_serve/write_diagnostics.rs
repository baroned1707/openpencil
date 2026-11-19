//! Design feedback on external MCP writes.
//!
//! The in-app design agent learns what its last `batch_design` broke (a
//! clipped frame, a misspelled icon, unreadable text) straight from the tool
//! result. An external agent used to learn it only by calling
//! `get_design_quality` or `finalize_design`. This module appends the SAME
//! feedback (`op_chat_agent::design_agent_tools::post_write_diagnostics`, the
//! function the in-app wrapper calls) to the result of the six write tools
//! below, on both MCP transports (live editor and headless `--mcp`), because
//! both reach `process_message_with_applier_profiled`.
//!
//! The feedback is advisory. It is appended only when an APPLIED write left
//! something to report (a clean write returns the same bytes as before), it is
//! capped (`MCP_FEEDBACK_CAPS`), and a panic or error while computing it is
//! logged and swallowed: the write result is returned unchanged.

use std::panic::{catch_unwind, AssertUnwindSafe};

use op_chat_agent::design_agent_tools::{
    page_index_for_selector, post_write_diagnostics, IconNameSnapshot, IconRename, PostWriteExtras,
    RenameRecording, MCP_FEEDBACK_CAPS,
};
use op_editor_core::{EditorCommand, EditorState};
use op_mcp::{RequestId, ToolCall, ToolRegistry, ToolResponse};
use serde_json::{Map, Value};

use super::wire::mcp_response_succeeded;
use super::McpServeError;

/// The write tools whose result carries design feedback.
const FEEDBACK_TOOLS: [&str; 6] = [
    "batch_design",
    "design_content",
    "design_refine",
    "insert_node",
    "replace_node",
    "update_node",
];

pub(super) fn wants_feedback(tool: &str) -> bool {
    FEEDBACK_TOOLS.contains(&tool)
}

/// Dispatch one tool call through `registry`, applying its command with
/// `apply`, and append design feedback when `feedback_call` (the parsed call,
/// only for a feedback tool) applied a write.
pub(super) fn dispatch<F>(
    registry: &ToolRegistry,
    line: &str,
    state: &mut EditorState,
    feedback_call: Option<ToolCall>,
    mut apply: F,
) -> Result<Option<String>, McpServeError>
where
    F: FnMut(&str, &mut EditorState, &EditorCommand) -> bool,
{
    // Icons as they stood before the write, and a log of any glyph name the
    // write's own repairs correct: on the headless transport that happens
    // inside `apply`, before the diagnostics below can see the old name.
    let icon_names_before = feedback_call
        .is_some()
        .then(|| IconNameSnapshot::capture(state));
    let recording = feedback_call.is_some().then(RenameRecording::start);
    let mut applied = false;
    let response = super::process_tool_message_with_registry(registry, line, |tool_name, cmd| {
        let ok = apply(tool_name, state, cmd);
        applied |= ok;
        ok
    })?;
    match (feedback_call, response, icon_names_before, recording) {
        (Some(call), Some(response), Some(before), Some(recording))
            if applied && !too_costly_for_small_writer(&call, state) =>
        {
            let selector = page_selector(&call);
            Ok(Some(append_feedback(response, &call.id, || {
                diagnose(state, selector.as_deref(), &before, &recording)
            })))
        }
        (_, response, _, _) => Ok(response),
    }
}

/// Pages above this many nodes skip the feedback for `update_node`: the
/// diagnostics clone and repair the page (about 140 ms on 2,000 nodes in a
/// debug build), which an agent pays on every one of its many small edits.
/// Whole-tree writers keep the feedback at any size.
pub(super) const SMALL_WRITER_NODE_LIMIT: usize = 600;

fn too_costly_for_small_writer(call: &ToolCall, state: &EditorState) -> bool {
    too_costly_for_tool(&call.tool, state)
}

#[cfg(test)]
pub(super) fn too_costly_for_small_writer_for_tests(tool: &str, state: &EditorState) -> bool {
    too_costly_for_tool(tool, state)
}

fn too_costly_for_tool(tool: &str, state: &EditorState) -> bool {
    use op_editor_core::pen_node_ext::PenNodeExt;
    if tool != "update_node" {
        return false;
    }
    fn count_up_to(nodes: &[jian_ops_schema::node::PenNode], limit: usize) -> usize {
        let mut total = 0;
        for node in nodes {
            total += 1;
            if total > limit {
                return total;
            }
            if let Some(children) = node.children() {
                total += count_up_to(children, limit - total);
                if total > limit {
                    return total;
                }
            }
        }
        total
    }
    count_up_to(state.active_children(), SMALL_WRITER_NODE_LIMIT) > SMALL_WRITER_NODE_LIMIT
}

/// The page a write names, under the same three spellings the command applier
/// accepts.
fn page_selector(call: &ToolCall) -> Option<String> {
    ["pageId", "page_id", "page"]
        .iter()
        .find_map(|key| call.arguments.get(*key))
        .map(|page| page.trim().to_string())
        .filter(|page| !page.is_empty())
}

/// The feedback keys for the page the write landed on. Works on a copy that
/// has had the write repairs, so it reports what the document looks like
/// after the host's own cleanup (the live path applies the command to a local
/// snapshot only; the host repairs the real document separately).
fn diagnose(
    state: &EditorState,
    selector: Option<&str>,
    icon_names_before: &IconNameSnapshot,
    recording: &RenameRecording,
) -> Result<Map<String, Value>, String> {
    let page = page_index_for_selector(state, selector)
        .ok_or_else(|| "write page is not on the document".to_string())?;
    let mut scratch = state.clone();
    scratch.ui.active_page_index = page;
    super::run_write_repairs_after_apply(&mut scratch, icon_names_before);
    // Renames made by the applier (headless) and by the pass above (live,
    // where the applier only touched a local snapshot) read the same.
    let icon_notes = recording.take().iter().map(IconRename::note).collect();
    Ok(post_write_diagnostics(
        &scratch,
        PostWriteExtras {
            icon_notes,
            ..PostWriteExtras::default()
        },
        Some(&MCP_FEEDBACK_CAPS),
    ))
}

/// `response` with `fields` appended to its JSON result. Unchanged (the very
/// same string) when the response is an error, `diagnose` has nothing to say,
/// or `diagnose` fails or panics.
pub(super) fn append_feedback(
    response: String,
    id: &RequestId,
    diagnose: impl FnOnce() -> Result<Map<String, Value>, String>,
) -> String {
    if !mcp_response_succeeded(&response) {
        return response;
    }
    let fields = match catch_unwind(AssertUnwindSafe(diagnose)) {
        Ok(Ok(fields)) => fields,
        Ok(Err(error)) => {
            eprintln!("openpencil-desktop mcp: design feedback skipped: {error}");
            return response;
        }
        Err(_) => {
            eprintln!("openpencil-desktop mcp: design feedback panicked; write result unchanged");
            return response;
        }
    };
    if fields.is_empty() {
        return response;
    }
    merge_into_result(&response, id, fields).unwrap_or(response)
}

/// Rebuild the success envelope with `fields` added to the tool's JSON text.
/// `None` when the result is not a single JSON-object text block (nothing is
/// appended then). Keys the tool already returned are never overwritten.
fn merge_into_result(
    response: &str,
    id: &RequestId,
    mut fields: Map<String, Value>,
) -> Option<String> {
    let envelope: Value = serde_json::from_str(response).ok()?;
    let blocks = envelope.get("result")?.get("content")?.as_array()?;
    let [block] = blocks.as_slice() else {
        return None;
    };
    let text = block.get("text")?.as_str()?.trim();
    let existing: Value = serde_json::from_str(text).ok()?;
    fields.retain(|key, _| existing.get(key).is_none());
    if !existing.is_object() || fields.is_empty() {
        return None;
    }
    // Splice textually so the tool's own bytes (key order, number formatting)
    // are untouched; only the new keys are added before the closing brace.
    let appended = serde_json::to_string(&fields).ok()?;
    let appended = appended.strip_prefix('{')?.strip_suffix('}')?;
    let body = text.strip_prefix('{')?.strip_suffix('}')?.trim();
    let merged = if body.is_empty() {
        format!("{{{appended}}}")
    } else {
        format!("{{{body},{appended}}}")
    };
    Some(op_mcp::tool_response_to_json(&ToolResponse::Ok {
        id: id.clone(),
        result: Default::default(),
        command: None,
        json: Some(merged),
        image: None,
    }))
}

#[cfg(test)]
#[path = "write_diagnostics_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "write_diagnostics_icon_rename_tests.rs"]
mod icon_rename_tests;
