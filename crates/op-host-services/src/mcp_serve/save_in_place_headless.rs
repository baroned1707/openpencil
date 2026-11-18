//! `save_document` WITHOUT a `filePath`, or with a `filePath` that is the
//! loaded file itself, in the headless `--mcp` / `--mcp-http` modes: the loaded
//! file is saved in place. Any other `filePath` is a copy, as before.
//!
//! A headless server owns its file outright and already writes it through
//! after every successful write (`save_editor_state`, atomic), so this changes
//! nothing about when the file is written; it only gives the call the same
//! meaning and answer shape it has on the live editor
//! (`{ saved, path, bytes, documentId, dirtyBefore }`). There is no editor
//! behind the file, so there is no disk fingerprint to check and nothing is
//! ever dirty.

use std::path::Path;

use op_editor_core::EditorState;

use super::document_address::{format_document_id, HEADLESS_DOCUMENT};
use super::tool_profile::McpAccessProfile;
use crate::disk_fingerprint::DiskFingerprint;
use crate::save_in_place::{is_in_place_save_call, own_path_claim, result_json, SavedInPlace};

/// The answer to a `save_document` for the loaded file, or `None` for any other call
/// (or one `profile` refuses, which the dispatcher reports).
pub(super) fn answer_in_headless(
    line: &str,
    path: &Path,
    state: &EditorState,
    profile: McpAccessProfile,
) -> Option<String> {
    let call = op_mcp::parse_tool_call(line.trim())?;
    let names_loaded_file = own_path_claim(&call)
        .is_some_and(|claimed| super::file_path::same_path(Path::new(claimed), path));
    if !(is_in_place_save_call(&call) || names_loaded_file)
        || profile.refuse_call(&call.tool, &call.arguments).is_some()
    {
        return None;
    }
    let response = match super::save_editor_state(state, path) {
        Ok(()) => {
            let fingerprint = DiskFingerprint::capture(path).unwrap_or(DiskFingerprint::Missing);
            let saved = SavedInPlace {
                path: path.to_path_buf(),
                bytes: match fingerprint {
                    DiskFingerprint::Present { len, .. } => len,
                    DiskFingerprint::Missing => 0,
                },
                dirty_before: false,
                fingerprint,
            };
            op_mcp::ToolResponse::Ok {
                id: call.id,
                result: Default::default(),
                command: None,
                json: Some(result_json(&saved, &format_document_id(HEADLESS_DOCUMENT))),
                image: None,
            }
        }
        Err(error) => op_mcp::ToolResponse::Err {
            id: call.id,
            code: op_mcp::ToolErrorCode::ToolFailed,
            message: error.to_string(),
        },
    };
    Some(op_mcp::tool_response_to_json(&response))
}

#[cfg(test)]
#[path = "save_in_place_headless_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "save_in_place_catalog_tests.rs"]
mod catalog_tests;
