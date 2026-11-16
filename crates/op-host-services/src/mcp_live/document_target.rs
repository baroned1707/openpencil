//! Which document a request is about.
//!
//! The order is fixed and never consults a session or "whatever is open" when
//! the call could be ambiguous:
//!
//! 1. the id the endpoint is bound to (`/mcp/d/<N>`); a `documentId` argument
//!    that disagrees is refused, not preferred;
//! 2. the `documentId` argument;
//! 3. the only open document, when exactly one is open;
//! 4. otherwise the call is refused with the list of open documents.
//!
//! A catalog request (`tools/list`) is not a call about a document, so where
//! a call would be refused it falls back to the document on screen instead.

use super::*;
use crate::mcp_serve::document_address::parse_document_id;

/// How strictly an unaddressed request is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Addressing {
    /// A tool call: several open documents and no id is an error.
    Call,
    /// A catalog request: fall back to the document on screen.
    Catalog,
}

pub(super) fn resolve_document(
    bound: Option<DocumentId>,
    argument: Option<&str>,
    addressing: Addressing,
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
) -> Result<DocumentId, McpLiveError> {
    if let Some(bound) = bound {
        return match argument {
            Some(named) if parse_document_id(named) != Some(bound) => {
                Err(McpLiveError::DocumentIdConflict {
                    bound,
                    requested: named.to_string(),
                })
            }
            _ => Ok(bound),
        };
    }
    if let Some(named) = argument {
        // Whether the id is open is the UI thread's call (it owns the list);
        // a spelling that is no id at all can be refused here.
        return parse_document_id(named).ok_or_else(|| McpLiveError::UnknownDocument {
            requested: named.to_string(),
            open: request_documents(req_tx, wake_ui).unwrap_or_default(),
        });
    }
    let open = request_documents(req_tx, wake_ui)?;
    if let [only] = open.as_slice() {
        return Ok(only.id);
    }
    if addressing == Addressing::Catalog {
        if let Some(active) = open.iter().find(|listing| listing.active) {
            return Ok(active.id);
        }
    }
    Err(McpLiveError::DocumentRequired { open })
}

/// The document on screen, for the REST routes (whole-document sync, the
/// browser extension's snapshot ingest) that name no document: they mean the
/// canvas the user is looking at.
pub(super) fn active_document(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
) -> Result<DocumentId, McpLiveError> {
    let open = request_documents(req_tx, wake_ui)?;
    open.iter()
        .find(|listing| listing.active)
        .map(|listing| listing.id)
        .ok_or(McpLiveError::DocumentRequired { open })
}
