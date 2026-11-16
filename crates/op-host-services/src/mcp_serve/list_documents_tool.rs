//! `list_documents` for the headless modes: the one document, `d1`.
//!
//! The live desktop server answers this tool itself from the editor's
//! document registry (`mcp_live::list_documents_route`); this registry entry is
//! what `--mcp`, `--mcp-http` and serve-web serve, where exactly one document
//! is open.

use std::collections::BTreeMap;
use std::path::Path;

use op_editor_core::{DocumentTabStatus, EditorState};
use op_mcp::{McpTool, ToolOutcome};

use super::document_address::HEADLESS_DOCUMENT;
use super::document_listing::{listings_json, project_name_for, DocumentListing};
use super::tool_profile::McpAccessProfile;

pub struct ListDocuments {
    listing: DocumentListing,
}

pub fn list_documents_snapshot(state: &EditorState) -> ListDocuments {
    ListDocuments {
        listing: DocumentListing {
            id: HEADLESS_DOCUMENT,
            label: state
                .editor_ui
                .file_name_display
                .clone()
                .unwrap_or_else(|| "Untitled".to_string()),
            path: None,
            project: String::new(),
            dirty: state.is_dirty(),
            active: true,
            status: DocumentTabStatus::Idle,
        },
    }
}

/// The listing of a headless transport's one document, which is backed by
/// `path`: the real file name, path and project instead of the registry
/// tool's placeholders (that tool only sees the editor state, which does not
/// know the file it was loaded from).
fn backed_listing(path: &Path) -> DocumentListing {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    DocumentListing {
        id: HEADLESS_DOCUMENT,
        label: canonical
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string()),
        project: project_name_for(Some(&canonical)),
        path: Some(canonical),
        // Every write is saved before it is acknowledged, so the file IS the
        // document; the editor state's own flag only says it moved past the
        // revision it was loaded at.
        dirty: false,
        active: true,
        status: DocumentTabStatus::Idle,
    }
}

/// Answer a `list_documents` call for a transport backed by the file `path`
/// (`--mcp`, `--mcp-http`). `None` for any other call, and for a call the
/// profile refuses (the generic dispatch then reports the refusal).
pub fn answer_in_headless(line: &str, path: &Path, profile: McpAccessProfile) -> Option<String> {
    let call = op_mcp::parse_tool_call(line.trim())?;
    if call.tool != "list_documents" || profile.refuse_call(&call.tool, &call.arguments).is_some() {
        return None;
    }
    Some(op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Ok {
        id: call.id,
        result: Default::default(),
        command: None,
        json: Some(listings_json(&[backed_listing(path)])),
        image: None,
    }))
}

impl McpTool for ListDocuments {
    fn name(&self) -> &str {
        "list_documents"
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        ToolOutcome::OkJson(listings_json(std::slice::from_ref(&self.listing)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_reports_its_single_document_as_d1() {
        let state = EditorState::new();
        let ToolOutcome::OkJson(json) = list_documents_snapshot(&state).call(&BTreeMap::new())
        else {
            panic!("expected JSON");
        };
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value.as_array().map(Vec::len), Some(1));
        assert_eq!(value[0]["documentId"], "d1");
        assert_eq!(value[0]["active"], true);
    }
}

#[cfg(test)]
#[path = "list_documents_tool_tests.rs"]
mod headless_tests;
