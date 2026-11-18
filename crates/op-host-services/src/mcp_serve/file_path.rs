//! Per-call `filePath` routing for the desktop MCP transports.
//!
//! The TypeScript MCP treats `filePath` as the target document for
//! most tools. The Rust desktop transports are still started with a
//! primary document path, but when a tool call supplies a different
//! `filePath`, dispatch against that file for this call only.

use std::path::{Path, PathBuf};

use super::file_path_error::FilePathError;
use super::McpServeError;

/// The `filePath` spellings that mean "the live canvas", not a file. The live
/// server's path policy exempts exactly these (and only for `filePath`).
pub(crate) const LIVE_CANVAS_SENTINELS: &[&str] = &["live://canvas", "live://canvas/current"];

struct TargetFileCall {
    id: op_mcp::RequestId,
    tool: String,
    path: PathBuf,
}

/// `profile` is the session's: a call retargeted at another file is served
/// under the same catalog and deployment rules as one against the primary.
///
/// `current_path` is the transport's own backing document: a `filePath`
/// naming it falls through to that document (`Ok(None)`). `open_paths` are
/// documents some editor holds in memory; a `filePath` naming one of those is
/// REFUSED with an `isError` tool result, because loading it from disk,
/// editing and writing it back would bypass the editor and be overwritten by
/// its next Save. Both sides are canonicalized, so symlinks, `..` segments and
/// relative spellings of the same file all match. A target that does not exist
/// cannot match and behaves as before.
pub fn process_message_for_file_path_arg(
    current_path: Option<&Path>,
    open_paths: &[PathBuf],
    line: &str,
    profile: super::tool_profile::McpAccessProfile,
) -> Result<Option<String>, McpServeError> {
    let Some(target) = target_file_call(line, current_path)? else {
        return Ok(None);
    };
    if open_paths.iter().any(|open| same_path(&target.path, open)) {
        return Ok(Some(op_mcp::tool_response_to_json(
            &op_mcp::ToolResponse::Err {
                id: target.id,
                code: op_mcp::ToolErrorCode::ToolFailed,
                message: FilePathError::OpenInEditor { path: target.path }.to_string(),
            },
        )));
    }
    if target.tool == "open_document" && !target.path.exists() {
        create_empty_document(&target.path)?;
    }
    let mut target_state = super::load_or_initialize_editor_state(&target.path)?;
    let mut applier_failed: Option<String> = None;
    let response = super::process_message_with_applier_profiled(
        &mut target_state,
        line,
        profile,
        |_tool_name, state, cmd| {
            if !state.apply(cmd.clone()) {
                return false;
            }
            super::run_write_repairs_after_apply(state);
            if let Err(e) = super::save_editor_state(state, &target.path) {
                applier_failed = Some(format!("save {} failed: {e}", target.path.display()));
                return false;
            }
            true
        },
    )?;
    if let Some(msg) = applier_failed {
        eprintln!("openpencil-desktop mcp: {msg}");
    }
    Ok(response)
}

fn target_file_call(
    line: &str,
    current_path: Option<&Path>,
) -> Result<Option<TargetFileCall>, McpServeError> {
    let Some(call) = op_mcp::parse_tool_call(line.trim()) else {
        return Ok(None);
    };
    let source_arg = if call.tool == "save_document" {
        "sourceFilePath"
    } else {
        "filePath"
    };
    let Some(raw) = call
        .arguments
        .get(source_arg)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if LIVE_CANVAS_SENTINELS.contains(&raw) {
        return Ok(None);
    }
    let target = PathBuf::from(raw);
    if current_path.is_some_and(|current| same_path(&target, current)) {
        return Ok(None);
    }
    Ok(Some(TargetFileCall {
        id: call.id,
        tool: call.tool,
        path: target,
    }))
}

fn create_empty_document(path: &Path) -> Result<(), McpServeError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| McpServeError::Document(format!("create {}: {e}", parent.display())))?;
    }
    std::fs::write(
        path,
        "{\n  \"version\": \"1.0.0\",\n  \"children\": []\n}\n",
    )
    .map_err(|e| McpServeError::Document(format!("write {}: {e}", path.display())))
}

/// Whether two paths are the same existing file: symlinks, `..` segments and
/// relative spellings resolved by the OS. A path that does not exist matches
/// nothing.
pub(super) fn same_path(target: &Path, current: &Path) -> bool {
    match (target.canonicalize(), current.canonicalize()) {
        (Ok(target), Ok(current)) => target == current,
        _ => false,
    }
}

#[cfg(test)]
#[path = "file_path_tests.rs"]
mod tests;
