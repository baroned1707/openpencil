//! UI-thread request helpers — every `UiRequest` round-trip the connection
//! threads make (snapshot / list-pages / apply / screenshot / replace /
//! editor-meta), plus the shared ack timeout and small JSON helpers. Split
//! out of `mcp_live.rs` to keep the spine under the 800-line cap.

use std::time::Instant;

use super::request_ticket::{Cancel, CancelOnDrop, TicketState};
use super::*;

/// Every open document, freshly read on the UI thread.
pub(super) fn request_documents(
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<Vec<DocumentListing>, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::Documents { ack: ack_tx })
        .map_err(|_| {
            McpLiveError::UiThread("UI thread is not accepting MCP document requests".to_string())
        })?;
    wake_ui();
    recv_with_timeout(
        ack_rx.recv_timeout(req_tx.timing().ack_budget),
        "document-list",
    )
}

/// Turn a resolver refusal into the error the agent should read. An unknown
/// id lists the open documents, so one more round trip is spent here — only
/// on the failure path.
pub(super) fn resolve_failure(
    req_tx: &UiSender,
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

/// Whether a request changes the document. It decides what a caller may claim
/// when the UI thread started the request but never answered.
#[derive(Clone, Copy)]
pub(super) enum Effect {
    /// Nothing to undo: a request the UI thread dropped or half-ran is `HostBusy`.
    ReadOnly,
    /// The change may have landed: the honest answer is `OutcomeUnknown`.
    Mutating,
}

/// What one wait for an answer ended with.
enum Wait<T> {
    Answer(T),
    /// The UI side dropped the answer channel without answering.
    Closed,
    /// The time ran out.
    Elapsed,
}

/// Longest stretch a caller blocks before re-reading the clock, so a clock
/// that moves independently of the thread (a test's) is noticed promptly. A
/// real answer wakes the caller at once; this only paces the timeout check.
const CLOCK_POLL: Duration = Duration::from_millis(20);

/// Wait for an answer until `until` on the server's clock.
fn wait_until<T>(timing: &LiveTiming, ack_rx: &Receiver<T>, until: Instant) -> Wait<T> {
    loop {
        let remaining = until.saturating_duration_since(timing.now());
        if remaining.is_zero() {
            // An answer that landed at the line still counts.
            return match ack_rx.try_recv() {
                Ok(answer) => Wait::Answer(answer),
                Err(TryRecvError::Disconnected) => Wait::Closed,
                Err(TryRecvError::Empty) => Wait::Elapsed,
            };
        }
        match ack_rx.recv_timeout(remaining.min(CLOCK_POLL)) {
            Ok(answer) => return Wait::Answer(answer),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Wait::Closed,
        }
    }
}

/// Send one ticketed request and wait for its answer under the caller's
/// budget, with an outcome the agent can act on:
///
/// * answered in time: the answer;
/// * out of time and still unstarted: the request is cancelled so the pump
///   will never run it, and the caller reports [`McpLiveError::HostBusy`];
/// * out of time but already started: the pump will finish it, so wait the
///   grace for the real answer; with none, a mutation reports
///   [`McpLiveError::OutcomeUnknown`] and a read reports `HostBusy`.
///
/// `label` names the request in the "not accepting" / "channel closed" faults.
pub(super) fn round_trip<T>(
    req_tx: &UiSender,
    wake_ui: &UiWake,
    label: &str,
    effect: Effect,
    build: impl FnOnce(Arc<Ticket>, mpsc::SyncSender<T>) -> UiRequest,
) -> Result<T, McpLiveError> {
    let timing = req_tx.timing();
    let ticket = Ticket::new(timing.deadline());
    // Whatever way this function ends (an early return, a panic unwinding the
    // connection thread), a request nobody waits for any more must not run.
    let _cancel_if_abandoned = CancelOnDrop::new(&ticket);
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(build(Arc::clone(&ticket), ack_tx))
        .map_err(|_| {
            McpLiveError::UiThread(format!("UI thread is not accepting MCP {label} requests"))
        })?;
    wake_ui();
    match wait_until(timing, &ack_rx, ticket.deadline()) {
        Wait::Answer(answer) => return Ok(answer),
        // The pump drops an expired request without answering it.
        Wait::Closed if ticket.state() == TicketState::Cancelled => {
            return Err(McpLiveError::HostBusy)
        }
        Wait::Closed => {
            return Err(McpLiveError::UiThread(format!(
                "UI {label} ack channel closed"
            )))
        }
        Wait::Elapsed => {}
    }
    match ticket.cancel() {
        Cancel::Won | Cancel::AlreadyCancelled => return Err(McpLiveError::HostBusy),
        Cancel::TooLate => {}
    }
    timing.note_grace_wait();
    match wait_until(timing, &ack_rx, timing.now() + timing.grace) {
        Wait::Answer(answer) => Ok(answer),
        Wait::Closed | Wait::Elapsed => Err(match effect {
            Effect::ReadOnly => McpLiveError::HostBusy,
            Effect::Mutating => McpLiveError::OutcomeUnknown,
        }),
    }
}

/// Snapshot document `document` and learn which lifetime of it this is. The
/// returned [`DocumentEpoch`] must ride back on the call's apply (see
/// [`request_apply`]) so a swap in between is caught.
pub(super) fn request_snapshot(
    req_tx: &UiSender,
    wake_ui: &UiWake,
    document: DocumentId,
) -> Result<(EditorState, DocumentEpoch), McpLiveError> {
    let answer = round_trip(
        req_tx,
        wake_ui,
        "snapshot",
        Effect::ReadOnly,
        |ticket, ack| UiRequest::Snapshot {
            document,
            ticket,
            ack,
        },
    )?;
    match answer {
        Ok(SnapshotAck { state, epoch }) => Ok((state, epoch)),
        Err(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
    }
}

pub(super) fn request_list_pages(
    req_tx: &UiSender,
    wake_ui: &UiWake,
    document: DocumentId,
) -> Result<op_mcp::ListPages, McpLiveError> {
    let answer = round_trip(
        req_tx,
        wake_ui,
        "page-list",
        Effect::ReadOnly,
        |ticket, ack| UiRequest::ListPages {
            document,
            ticket,
            ack,
        },
    )?;
    match answer {
        Ok(pages) => Ok(pages),
        Err(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
    }
}

/// Ask the UI thread to apply `cmd` to `document`. `expected_epoch` is the
/// epoch from the call's snapshot of that document (`None` for a call that
/// took none); when the document no longer matches it the apply is refused
/// and this reports [`McpLiveError::DocumentChanged`].
pub(super) fn request_apply(
    req_tx: &UiSender,
    wake_ui: &UiWake,
    document: DocumentId,
    tool_name: String,
    cmd: EditorCommand,
    expected_epoch: Option<DocumentEpoch>,
) -> Result<ApplyAck, McpLiveError> {
    let ack = round_trip(req_tx, wake_ui, "apply", Effect::Mutating, |ticket, ack| {
        UiRequest::Apply {
            document,
            tool_name,
            cmd,
            expected_epoch,
            ticket,
            ack,
        }
    })?;
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
    req_tx: &UiSender,
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
    req_tx: &UiSender,
    wake_ui: &UiWake,
    doc: jian_ops_schema::PenDocument,
    editor_meta: op_pen_loader::EditorMeta,
) -> Result<bool, McpLiveError> {
    round_trip(
        req_tx,
        wake_ui,
        "document-sync",
        Effect::Mutating,
        |ticket, ack| UiRequest::ReplaceDocument {
            doc: Box::new(doc),
            editor_meta,
            ticket,
            ack,
        },
    )?
    .map_err(no_document_on_screen)
}

pub(super) fn request_update_editor_meta(
    req_tx: &UiSender,
    wake_ui: &UiWake,
    editor_meta: op_pen_loader::EditorMeta,
) -> Result<(), McpLiveError> {
    round_trip(
        req_tx,
        wake_ui,
        "metadata-sync",
        Effect::Mutating,
        |ticket, ack| UiRequest::UpdateEditorMeta {
            editor_meta,
            ticket,
            ack,
        },
    )?
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
