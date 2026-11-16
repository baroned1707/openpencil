//! `list_documents` for the headless modes: the one document, `d1`.
//!
//! The live desktop server answers this tool itself from the editor's
//! document registry (`mcp_live::list_documents_route`); this registry entry is
//! what `--mcp`, `--mcp-http` and serve-web serve, where exactly one document
//! is open.

use std::collections::BTreeMap;

use op_editor_core::{DocumentTabStatus, EditorState};
use op_mcp::{McpTool, ToolOutcome};

use super::document_address::HEADLESS_DOCUMENT;
use super::document_listing::{listings_json, DocumentListing};

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
