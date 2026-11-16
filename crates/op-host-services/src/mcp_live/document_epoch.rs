//! Document identity for the two halves of a live tool call.
//!
//! A live tool call is a snapshot request followed by an apply request, and
//! node ids repeat across documents. Without an identity carried from the
//! first half to the second, an apply that lands after the on-screen document
//! was replaced would succeed against the WRONG document. The snapshot ack
//! therefore returns a [`DocumentEpoch`], the apply hands it back, and the UI
//! thread refuses the apply when the epoch no longer names the live document.

use super::*;

/// Identifies one lifetime of the on-screen document. It changes whenever the
/// document is replaced (Open, New, import, whole-document MCP sync, a
/// collaboration snapshot) and NEVER on an ordinary edit, save, or undo.
///
/// Two counters are combined because neither covers every replacement alone:
/// - `host`: the desktop host's `document_epoch`, bumped when the whole
///   `EditorState` is swapped (Open / New / import). A fresh state restarts
///   its own generation at 0, so the generation alone would alias.
/// - `generation`: [`EditorState::document_generation`], bumped by
///   `replace_document` — which the MCP document-sync arm calls directly,
///   without the host knowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DocumentEpoch {
    host: u64,
    generation: u64,
}

impl DocumentEpoch {
    pub(super) fn of(host_epoch: u64, state: &EditorState) -> Self {
        Self {
            host: host_epoch,
            generation: state.document_generation(),
        }
    }
}

/// What the UI thread answers a snapshot request with: the narrowed state copy
/// plus the identity of the document it was taken from.
pub(super) struct SnapshotAck {
    pub(super) state: EditorState,
    pub(super) epoch: DocumentEpoch,
}

/// The `isError` tool result for a call whose apply was refused because the
/// document changed underneath it. The applier can only answer `false`, which
/// the dispatcher would turn into a generic "host rejected command" message,
/// so the connection replaces that reply with this one. `None` when `line`
/// carries no recoverable tool call (the caller keeps its own reply).
pub(super) fn document_changed_response(line: &str) -> Option<String> {
    let call = op_mcp::parse_tool_call(line.trim())?;
    Some(op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Err {
        id: call.id,
        code: op_mcp::ToolErrorCode::ToolFailed,
        message: McpLiveError::DocumentChanged.to_string(),
    }))
}

#[cfg(test)]
#[path = "document_epoch_tests.rs"]
mod tests;
