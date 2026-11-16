//! One open document as an MCP agent sees it: the row `list_documents`
//! returns and the line an addressing refusal prints.
//!
//! The desktop fills these from its document registry; every headless mode
//! reports its single document as `d1`. The type is plain data so the live
//! server, the headless registry tool and the refusal messages all share one
//! description of "an open document".

use std::path::{Path, PathBuf};

use op_editor_core::{DocumentId, DocumentTabStatus};

use super::document_address::format_spoken_id;
use super::document_nonce::LaunchNonce;

/// One open document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentListing {
    pub id: DocumentId,
    /// File name, or the untitled placeholder.
    pub label: String,
    /// The file the document is bound to; `None` while untitled.
    pub path: Option<PathBuf>,
    /// Project folder name used to group documents; empty when unknown.
    pub project: String,
    pub dirty: bool,
    /// Whether this is the document on screen. Only that one can be edited
    /// today; the others are parked.
    pub active: bool,
    pub status: DocumentTabStatus,
    /// The launch nonce the live server stamps on every listing it hands out,
    /// so the id an agent copies is the full `d<N>.<nonce>`. `None` (what a
    /// resolver reports, and every headless mode) keeps the bare `d<N>`.
    pub nonce: Option<LaunchNonce>,
}

impl DocumentListing {
    /// The id an agent must use to address this document.
    pub fn spoken_id(&self) -> String {
        format_spoken_id(self.id, self.nonce)
    }
}

/// The project folder name used to group a document stored at `path`
/// (`project_root_for`), or empty for an untitled document or a path with no
/// usable project directory.
pub fn project_name_for(path: Option<&Path>) -> String {
    crate::project_root::project_root_for_document(path)
        .map(|root| root.name)
        .unwrap_or_default()
}

impl DocumentListing {
    /// `d3.a1b2c3 storefront/checkout.op (active)` — the form an agent can copy
    /// the id out of.
    pub fn describe(&self) -> String {
        let name = if self.project.is_empty() {
            self.label.clone()
        } else {
            format!("{}/{}", self.project, self.label)
        };
        let marker = if self.active { " (active)" } else { "" };
        format!("{} {name}{marker}", self.spoken_id())
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "documentId": self.spoken_id(),
            "label": self.label,
            "path": self.path.as_ref().map(|path| path.to_string_lossy()),
            "project": self.project,
            "dirty": self.dirty,
            "active": self.active,
            "status": status_name(self.status),
        })
    }
}

fn status_name(status: DocumentTabStatus) -> &'static str {
    match status {
        DocumentTabStatus::Idle => "idle",
        DocumentTabStatus::Running => "running",
        DocumentTabStatus::NeedsReview => "needs_review",
        DocumentTabStatus::Done => "done",
        DocumentTabStatus::Error => "error",
    }
}

/// The `list_documents` result: a JSON array with one object per document.
pub fn listings_json(listings: &[DocumentListing]) -> String {
    serde_json::Value::Array(listings.iter().map(DocumentListing::to_json).collect()).to_string()
}

/// `d3 a/b.op (active); d4 Untitled` for an error message, or a plain
/// statement when nothing is open.
pub fn describe_open(listings: &[DocumentListing]) -> String {
    if listings.is_empty() {
        return "no document is open".to_string();
    }
    listings
        .iter()
        .map(DocumentListing::describe)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(id: u64, label: &str, project: &str, active: bool) -> DocumentListing {
        DocumentListing {
            id: DocumentId(id),
            label: label.to_string(),
            path: (!project.is_empty()).then(|| PathBuf::from(format!("/w/{project}/{label}"))),
            project: project.to_string(),
            dirty: false,
            active,
            status: DocumentTabStatus::Idle,
            nonce: None,
        }
    }

    #[test]
    fn describe_names_project_file_and_the_active_marker() {
        assert_eq!(
            listing(3, "checkout.op", "storefront", true).describe(),
            "d3 storefront/checkout.op (active)"
        );
        assert_eq!(listing(4, "Untitled", "", false).describe(), "d4 Untitled");
    }

    #[test]
    fn a_live_listing_names_itself_with_the_launch_nonce() {
        let mut live = listing(3, "checkout.op", "storefront", true);
        live.nonce = LaunchNonce::parse("a1b2c3");
        assert_eq!(live.describe(), "d3.a1b2c3 storefront/checkout.op (active)");
        let json = listings_json(&[live]);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value[0]["documentId"], "d3.a1b2c3");
    }

    #[test]
    fn list_json_carries_every_documented_field() {
        let json = listings_json(&[
            listing(3, "checkout.op", "storefront", true),
            listing(4, "Untitled", "", false),
        ]);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value[0]["documentId"], "d3");
        assert_eq!(value[0]["path"], "/w/storefront/checkout.op");
        assert_eq!(value[0]["active"], true);
        assert_eq!(value[1]["path"], serde_json::Value::Null);
        assert_eq!(value[1]["status"], "idle");
        assert_eq!(value[1]["dirty"], false);
    }
}
