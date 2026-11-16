//! Which document a request is about.
//!
//! The order is fixed and never consults a session or "whatever is open" when
//! the call could be ambiguous:
//!
//! 1. the id the endpoint is bound to (`/mcp/d/<id>`); a `documentId` argument
//!    that disagrees is refused, not preferred;
//! 2. the `documentId` argument;
//! 3. the only open document, when exactly one is open;
//! 4. otherwise the call is refused with the list of open documents.
//!
//! An id from steps 1 and 2 is the full `d<N>.<nonce>` of this launch. The
//! bare `d<N>` is accepted only while it names the single open document, and
//! any other bare or older-launch id is a `StaleDocumentId` refusal.
//!
//! A catalog request (`tools/list`) is not a call about a document, so where
//! a call would be refused it falls back to the document on screen instead.

use super::*;
use crate::mcp_serve::document_address::{format_spoken_id, parse_document_ref, DocumentRef};

/// How strictly an unaddressed request is treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Addressing {
    /// A tool call: several open documents and no id is an error.
    Call,
    /// A catalog request: fall back to the document on screen.
    Catalog,
}

pub(super) fn resolve_document(
    bound: Option<DocumentRef>,
    argument: Option<&str>,
    addressing: Addressing,
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<DocumentId, McpLiveError> {
    let named = match (bound, argument) {
        (Some(bound), Some(argument)) => {
            if !parse_document_ref(argument).is_some_and(|given| given.agrees_with(&bound)) {
                return Err(McpLiveError::DocumentIdConflict {
                    bound: bound.id,
                    nonce: bound.nonce,
                    requested: argument.to_string(),
                });
            }
            Some((bound, argument.to_string()))
        }
        (Some(bound), None) => Some((bound, format_spoken_id(bound.id, bound.nonce))),
        // Whether the id is open is the UI thread's call (it owns the list);
        // a spelling that is no id at all can be refused here.
        (None, Some(argument)) => match parse_document_ref(argument) {
            Some(given) => Some((given, argument.to_string())),
            None => {
                return Err(McpLiveError::UnknownDocument {
                    requested: argument.to_string(),
                    open: request_documents(req_tx, wake_ui).unwrap_or_default(),
                })
            }
        },
        (None, None) => None,
    };
    if let Some((given, spelled)) = named {
        match vet_reference(given, &spelled, req_tx, wake_ui) {
            Ok(id) => return Ok(id),
            // A catalog is not about a document, so a stale id does not stop
            // a client from listing the tools; it falls back to the screen.
            Err(McpLiveError::StaleDocumentId { .. }) if addressing == Addressing::Catalog => {}
            Err(error) => return Err(error),
        }
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

/// Accept `given` only when it can only mean the document the agent saw.
///
/// * the full form must carry THIS launch's nonce (an older launch's id is
///   refused even when its number is open again);
/// * the bare `d<N>` of the old spelling is honoured only while exactly one
///   document is open and it is that one: with several open it is ambiguous
///   across a restart, and with another one open it is stale.
fn vet_reference(
    given: DocumentRef,
    spelled: &str,
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<DocumentId, McpLiveError> {
    match given.nonce {
        Some(nonce) if nonce == req_tx.nonce() => Ok(given.id),
        Some(_) => Err(McpLiveError::StaleDocumentId {
            requested: spelled.to_string(),
            open: request_documents(req_tx, wake_ui)?,
        }),
        None => {
            let open = request_documents(req_tx, wake_ui)?;
            match open.as_slice() {
                [only] if only.id == given.id => Ok(given.id),
                _ => Err(McpLiveError::StaleDocumentId {
                    requested: spelled.to_string(),
                    open,
                }),
            }
        }
    }
}

/// The document on screen, for the REST routes (whole-document sync, the
/// browser extension's snapshot ingest) that name no document: they mean the
/// canvas the user is looking at.
pub(super) fn active_document(
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<DocumentId, McpLiveError> {
    let open = request_documents(req_tx, wake_ui)?;
    open.iter()
        .find(|listing| listing.active)
        .map(|listing| listing.id)
        .ok_or(McpLiveError::DocumentRequired { open })
}
