//! UI-thread request helpers — every `UiRequest` round-trip the connection
//! threads make (snapshot / list-pages / apply / screenshot / replace /
//! editor-meta), plus the shared ack timeout and small JSON helpers. Split
//! out of `mcp_live.rs` to keep the spine under the 800-line cap.

use super::*;

/// Every open document, freshly read on the UI thread.
pub(super) fn request_documents(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
) -> Result<Vec<DocumentListing>, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Documents { ack: ack_tx })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP document requests".to_string())
        })?;
    wake_ui();
    recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "document-list")
}

/// Turn a resolver refusal into the error the agent should read. An unknown
/// id lists the open documents, so one more round trip is spent here — only
/// on the failure path.
pub(super) fn resolve_failure(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    document: DocumentId,
    reason: ResolveError,
) -> McpLiveError {
    match reason {
        ResolveError::NotReachable => McpLiveError::DocumentNotOnScreen { document },
        ResolveError::Unknown => match request_documents(req_tx, wake_ui) {
            Ok(open) => McpLiveError::UnknownDocument {
                requested: crate::mcp_serve::document_address::format_document_id(document),
                open,
            },
            Err(error) => error,
        },
    }
}

/// Snapshot document `document` and learn which lifetime of it this is. The
/// returned [`DocumentEpoch`] must ride back on the call's apply (see
/// [`request_apply`]) so a swap in between is caught.
pub(super) fn request_snapshot(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    document: DocumentId,
) -> Result<(EditorState, DocumentEpoch), McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Snapshot {
            document,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP snapshot requests".to_string())
        })?;
    wake_ui();
    match recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "snapshot")? {
        Ok(SnapshotAck { state, epoch }) => Ok((state, epoch)),
        Err(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
    }
}

pub(super) fn request_list_pages(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    document: DocumentId,
) -> Result<op_mcp::ListPages, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::ListPages {
            document,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP page-list requests".to_string())
        })?;
    wake_ui();
    match recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "page-list")? {
        Ok(pages) => Ok(pages),
        Err(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
    }
}

/// Ask the UI thread to apply `cmd` to `document`. `expected_epoch` is the
/// epoch from the call's snapshot of that document (`None` for a call that
/// took none); when the document no longer matches it the apply is refused
/// and this reports [`McpLiveError::DocumentChanged`].
pub(super) fn request_apply(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    document: DocumentId,
    tool_name: String,
    cmd: EditorCommand,
    expected_epoch: Option<DocumentEpoch>,
) -> Result<ApplyAck, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Apply {
            document,
            tool_name,
            cmd,
            expected_epoch,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP apply requests".to_string())
        })?;
    wake_ui();
    let ack = recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "apply")?;
    if let Some(reason) = ack.unresolved {
        return Err(resolve_failure(req_tx, wake_ui, document, reason));
    }
    if ack.document_changed {
        return Err(McpLiveError::DocumentChanged);
    }
    Ok(ack)
}

/// Ask the UI thread to render a `debug_screenshot` capture of `document`.
/// Bounded wait per the request's own budget (TS `min(timeoutMs ?? 15000,
/// 60000)`) — the same recv-timeout discipline the chat canvas tools
/// use, so a stalled UI pump can never pin the connection thread.
#[cfg(feature = "mcp-debug-tools")]
pub(super) fn request_screenshot(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    document: DocumentId,
    req: op_mcp::ScreenshotRequest,
) -> Result<crate::export::screenshot::ScreenshotPng, McpLiveError> {
    let timeout = Duration::from_millis(req.timeout_ms.max(1));
    let spec = screenshot::capture_spec(&req);
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Screenshot {
            document,
            spec,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP screenshot requests".to_string())
        })?;
    wake_ui();
    // Three layers: the outer `?` is the UI round-trip (channel closed / ack
    // timed out), the middle is the document lookup, the inner `?` is the
    // raster capture itself — all land in `McpLiveError`, the export one via
    // `From<ExportError>`.
    match recv_with_timeout(ack_rx.recv_timeout(timeout), "screenshot")? {
        Ok(capture) => Ok(capture?),
        Err(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
    }
}

/// Ask the UI thread to swap an already-loaded document into the one on
/// screen. `Ok(false)` means collaboration rejected the replacement; `Err` is
/// a channel/timeout fault. Load/validation failures are handled before this
/// call and surface as 400.
pub(super) fn request_replace(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    doc: jian_ops_schema::PenDocument,
    editor_meta: op_pen_loader::EditorMeta,
) -> Result<bool, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::ReplaceDocument {
            doc: Box::new(doc),
            editor_meta,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread(
                "UI thread is not accepting MCP document-sync requests".to_string(),
            )
        })?;
    wake_ui();
    recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "document-sync")?
        .map_err(no_document_on_screen)
}

pub(super) fn request_update_editor_meta(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    editor_meta: op_pen_loader::EditorMeta,
) -> Result<(), McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::UpdateEditorMeta {
            editor_meta,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread(
                "UI thread is not accepting MCP metadata-sync requests".to_string(),
            )
        })?;
    wake_ui();
    recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "metadata-sync")?
        .map_err(no_document_on_screen)
}

/// The REST sync routes target "the document on screen"; a host that cannot
/// lend one has nothing to sync into.
fn no_document_on_screen(_: ResolveError) -> McpLiveError {
    McpLiveError::UiThread("no document is on screen to sync into".to_string())
}

pub(super) fn recv_with_timeout<T>(
    result: Result<T, RecvTimeoutError>,
    label: &str,
) -> Result<T, McpLiveError> {
    match result {
        Ok(v) => Ok(v),
        Err(RecvTimeoutError::Timeout) => Err(McpLiveError::UiThread(format!(
            "timed out waiting for UI {label} ack"
        ))),
        Err(RecvTimeoutError::Disconnected) => Err(McpLiveError::UiThread(format!(
            "UI {label} ack channel closed"
        ))),
    }
}

/// Build a per-instance identity token. Not security-sensitive — it only
/// needs to be distinct across editor instances so the CLI can tell a
/// live server apart from a stale discovery file or a recycled port. pid
/// + start nanos is unique enough (two instances never start at the same
///   nanosecond under the same pid).
pub(super) fn make_live_token() -> String {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{pid:x}-{nanos:x}")
}

/// Live `ping` reply: OpenPencil identity + `mode:"live"` + the instance
/// token, so `op` can distinguish this live canvas server from the
/// headless file server and confirm it owns the discovery file. The
/// identity rides in `_meta` — the MCP spec's extension point — because
/// strict clients validate a ping result as exactly `{ _meta? }` and
/// reject top-level extras (Gemini CLI, issue #199); see
/// `mcp_serve::ping_response`.
pub(super) fn live_ping_response(id_raw: &str, token: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id_raw},"result":{{"_meta":{{"server":"{}","mode":"live","token":"{}"}}}}}}"#,
        crate::mcp_serve::MCP_SERVER_NAME,
        crate::mcp_serve::json_escape(token)
    )
}

pub(super) fn error_json(message: &str) -> String {
    format!(
        r#"{{"error":"{}"}}"#,
        crate::mcp_serve::json_escape(message)
    )
}
