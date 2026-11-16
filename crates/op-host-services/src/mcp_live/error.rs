//! Typed failures for the in-process live-GUI MCP server (`mcp_live.rs`):
//! start-up, the per-connection handlers, and every UI-thread round-trip the
//! connection threads make.
//!
//! Style follows `op_orchestrator::OrchestratorError`: a plain enum plus a
//! hand-written `Display`, no `thiserror` and no new dependency. `Display` is
//! transparent — each variant reproduces the exact sentence the stringly
//! code produced, so the stderr lines (`openpencil-desktop mcp: …`), the
//! `{"error":…}` HTTP bodies, and the `Renderer reported failure: …`
//! screenshot envelope are unchanged byte for byte.
//!
//! What the enum adds is the classification this module never had: a UI-ack
//! timeout, a refused MCP message, and a failed raster capture used to be
//! indistinguishable `String`s that the accept loop dumped to stderr and the
//! 500 handler echoed. Two of them now arrive pre-classified from the shared
//! layers ([`McpServeError`], [`ExportError`]) through the `From` impls
//! below, which is what let `mcp_serve`'s three `String` seams
//! (`write_mcp_http_response`, `process_tool_message_with_registry`,
//! `doc_sync`'s validation) and `export`'s `From<ExportError> for String`
//! bridge be deleted — this module was their last stringly consumer.

use std::fmt;

use op_editor_core::DocumentId;

use crate::export::ExportError;
use crate::mcp_serve::document_address::format_spoken_id;
use crate::mcp_serve::document_listing::{describe_open, DocumentListing};
use crate::mcp_serve::document_nonce::LaunchNonce;
use crate::mcp_serve::McpServeError;

use super::path_policy::PathRefusal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpLiveError {
    /// Start-up failed: the listener would not bind, its bound port was
    /// unreadable, or the server thread would not spawn. Never becomes an
    /// HTTP response — it aborts `McpLiveServer::start_with_wake`, and the
    /// desktop host falls back to another port or gives up.
    Startup(String),
    /// A UI-thread round-trip failed: the request channel is closed (the
    /// editor is shutting down) or the ack did not arrive inside the
    /// per-request budget (the caller's own budget for screenshots and document
    /// lists). A server fault — the wire answers 500. Mutations and snapshots
    /// that time out are `HostBusy` / `OutcomeUnknown` instead.
    UiThread(String),
    /// The shared MCP transport refused the message or the socket write
    /// failed. Carries [`McpServeError`] so the classification the transport
    /// already made (client fault vs dead connection) survives the hop.
    Transport(McpServeError),
    /// The live raster capture behind `debug_screenshot` failed. Carries
    /// [`ExportError`] so the screenshot glue can report the renderer's own
    /// reason inside its `Renderer reported failure: …` envelope.
    Screenshot(ExportError),
    /// The on-screen document was replaced (Open, New, import, whole-document
    /// sync) between a tool call's snapshot and its apply, so the apply was
    /// refused rather than landed on a different document that happens to
    /// reuse the same node ids. A client-visible tool error: the agent should
    /// re-read the document and retry.
    DocumentChanged,
    /// Several documents are open (or none) and the call named none. The
    /// server never guesses which one an agent meant.
    DocumentRequired { open: Vec<DocumentListing> },
    /// The named `documentId` is not an open document (closed, never
    /// existed, or not the `d<N>` spelling).
    UnknownDocument {
        requested: String,
        open: Vec<DocumentListing>,
    },
    /// The id is the bare `d<N>` of the old spelling while several documents
    /// are open (or it is not the only open document's), or it carries another
    /// launch's nonce. Ids restart at 1 on every launch, so honouring it could
    /// silently edit a different document than the agent saw.
    StaleDocumentId {
        requested: String,
        open: Vec<DocumentListing>,
    },
    /// The document is open but the host cannot lend its state right now: it
    /// is parked behind the document on screen.
    DocumentNotOnScreen {
        document: DocumentId,
        nonce: Option<LaunchNonce>,
    },
    /// The call arrived on an endpoint bound to one document
    /// (`/mcp/d/<id>`) yet named a different one in `documentId`.
    DocumentIdConflict {
        bound: DocumentId,
        nonce: Option<LaunchNonce>,
        requested: String,
    },
    /// A path argument leaves the folders an agent may use (see
    /// `path_policy`). Nothing was read, written or created.
    PathNotAllowed {
        tool: String,
        argument: String,
        path: String,
        reason: PathRefusal,
        /// What the agent may use instead.
        roots: Vec<std::path::PathBuf>,
    },
    /// An `operationId` was reused for a different call.
    OperationIdConflict { operation_id: String },
    /// The UI thread did not start the request inside the caller's budget (a
    /// modal dialog, a picker or a save is holding it). The request was
    /// cancelled before it ran and the pump will never apply it, so retrying
    /// with the same `operationId` is safe.
    HostBusy,
    /// The UI thread had already started a mutation when the caller's budget
    /// ran out and never answered within the grace. The change may or may not
    /// have landed, so a retry is refused until the agent has re-read the
    /// document.
    OutcomeUnknown,
}

impl McpLiveError {
    /// Whether the client should see this as a tool error it can act on
    /// (answered `200` with `isError:true`) rather than a server fault (500).
    pub(super) fn is_tool_error(&self) -> bool {
        matches!(
            self,
            McpLiveError::DocumentChanged
                | McpLiveError::DocumentRequired { .. }
                | McpLiveError::UnknownDocument { .. }
                | McpLiveError::StaleDocumentId { .. }
                | McpLiveError::PathNotAllowed { .. }
                | McpLiveError::DocumentNotOnScreen { .. }
                | McpLiveError::DocumentIdConflict { .. }
                | McpLiveError::OperationIdConflict { .. }
                | McpLiveError::HostBusy
                | McpLiveError::OutcomeUnknown
        )
    }
}

impl fmt::Display for McpLiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            McpLiveError::Startup(m) | McpLiveError::UiThread(m) => f.write_str(m),
            McpLiveError::Transport(e) => e.fmt(f),
            McpLiveError::Screenshot(e) => e.fmt(f),
            McpLiveError::DocumentChanged => f.write_str(
                "the open document changed while this call was running; re-read and retry",
            ),
            McpLiveError::DocumentRequired { open } => write!(
                f,
                "this call does not say which document it is about and the choice is not \
                 obvious. Pass `documentId` (or call list_documents). Open documents: {}",
                describe_open(open)
            ),
            McpLiveError::UnknownDocument { requested, open } => write!(
                f,
                "unknown documentId '{requested}'. Open documents: {}. Pass one of these ids \
                 or call list_documents",
                describe_open(open)
            ),
            McpLiveError::StaleDocumentId { requested, open } => write!(
                f,
                "documentId '{requested}' is not a current document id: it belongs to a \
                 previous session or is incomplete (the full form is d<N>.<nonce>, and the \
                 bare d<N> is accepted only while it is the single open document). Open \
                 documents: {}. Pass one of these ids or call list_documents",
                describe_open(open)
            ),
            McpLiveError::PathNotAllowed {
                tool,
                argument,
                path,
                reason,
                roots,
            } => {
                write!(f, "{tool}: {argument} '{path}' was refused: {reason}.")?;
                if roots.is_empty() {
                    return Ok(());
                }
                let roots: Vec<String> = roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect();
                write!(
                    f,
                    " Use an absolute path inside one of: {}",
                    roots.join(", ")
                )
            }
            McpLiveError::DocumentNotOnScreen { document, nonce } => write!(
                f,
                "document {} is open but not on screen yet; switch to it",
                format_spoken_id(*document, *nonce)
            ),
            McpLiveError::DocumentIdConflict {
                bound,
                nonce,
                requested,
            } => write!(
                f,
                "documentId '{requested}' conflicts with the document this endpoint is bound \
                 to ({}); omit documentId or use the /mcp endpoint",
                format_spoken_id(*bound, *nonce)
            ),
            McpLiveError::OperationIdConflict { operation_id } => write!(
                f,
                "operationId '{operation_id}' was already used for a different call; use a new \
                 operationId for a new call"
            ),
            McpLiveError::HostBusy => f.write_str(
                "the editor did not respond in time (a dialog may be open); the change was NOT \
                 applied — retry",
            ),
            McpLiveError::OutcomeUnknown => f.write_str(
                "the change may or may not have been applied; read the document before retrying",
            ),
        }
    }
}

impl std::error::Error for McpLiveError {}

/// Lets every `mcp_serve` primitive the connection handlers reach for
/// (`read_http_request`, `write_mcp_http_response`,
/// `process_message_with_applier`, `process_tool_message_with_registry`,
/// `parse_document_sync_request`, `file_path::process_message_for_file_path_arg`)
/// be used with a plain `?`. Both `Display` impls are transparent, so the
/// resulting log line / response body is byte-identical to the
/// pre-conversion text.
impl From<McpServeError> for McpLiveError {
    fn from(error: McpServeError) -> McpLiveError {
        McpLiveError::Transport(error)
    }
}

/// Same for the UI pump's raster capture, so `request_screenshot` can hand
/// the export core's reason straight to the screenshot wire glue.
impl From<ExportError> for McpLiveError {
    fn from(error: ExportError) -> McpLiveError {
        McpLiveError::Screenshot(error)
    }
}
