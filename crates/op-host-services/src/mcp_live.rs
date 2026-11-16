//! In-process MCP HTTP server for the live desktop editor.
//!
//! The CLI-only `mcp_serve` path owns a file-backed `EditorState`.
//! This module keeps the GUI as the source of truth: the server thread
//! requests a fresh snapshot from the UI thread for each HTTP request,
//! then sends write commands back for the UI thread to apply.

use std::collections::{HashSet, VecDeque};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jian_ops_schema::node::PenNode;
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::{
    CollabDocumentMutation, CollabEditSource, CollabGateAction, CollabGatePolicy,
    CollabUnsupportedFeature, DocumentId, EditorCommand, EditorState,
};

pub mod error;
#[cfg(feature = "mcp-debug-tools")]
pub mod screenshot;

/// The URL an agent spawned for one document should be given.
pub use crate::mcp_serve::document_address::mcp_url_for_document;
pub use design_md_route::{DesignMdResponder, DesignMdResponseError, PendingDesignMdRequest};
pub use error::McpLiveError;
pub use live_timing::{Clock, SystemClock};
pub use path_policy::PathRefusal;
pub use resolver::{DocumentListing, DocumentResolver, ResolveError, SingleDocumentResolver};

/// Per-request budget for a UI-thread snapshot/apply ack. Sized to cover a
/// large single editor operation without the connection giving up; the CLI
/// allows even longer per tool call (`POST_TIMEOUT`).
const UI_ACK_TIMEOUT: Duration = Duration::from_secs(15);
const ACCEPT_IDLE_SLEEP: Duration = Duration::from_millis(25);
/// Cap on concurrently-served connections, so a flood of localhost connects
/// can't exhaust threads/memory. Excess connections are shed with a 503.
const MAX_LIVE_CONNS: usize = 64;
/// Maximum UI-thread MCP requests to drain in one redraw. A client that
/// enqueues a large burst should not be able to monopolize the frame.
const UI_PUMP_REQUEST_BUDGET: usize = 8;
/// Per-request HTTP connection stack. Deep JSON deserialization is handled by
/// `serde_stacker`; reserving hundreds of MB per short-lived localhost request
/// makes large live-canvas syncs look like runaway desktop memory growth.
const LIVE_CONN_STACK_SIZE: usize = 16 * 1024 * 1024;
const _: () = assert!(LIVE_CONN_STACK_SIZE <= 16 * 1024 * 1024);
type UiWake = Arc<dyn Fn() + Send + Sync + 'static>;

/// Fallback client-facing label when `initialize` carried no
/// `clientInfo.name` (some minimal/older MCP clients omit it) — honest
/// about what it is ("some external MCP tool"), not a fabricated persona.
const MCP_CLIENT_FALLBACK_NAME: &str = "MCP Client";
/// Fixed neutral badge colour for every MCP-driven generation — a
/// deliberate contrast with the in-app agent persona pool
/// (`assign_agent_identities_seeded`'s vivid per-agent colours): an
/// external MCP client isn't one of the app's own named agents.
const MCP_CLIENT_COLOR: &str = "#8A8F98";

/// What a live server starts with beyond its port and wake callback. The
/// default is the production setting: this launch's nonce, no bearer token, no
/// extra folders.
#[derive(Clone, Default)]
pub struct LiveOptions {
    /// Names the documents with this nonce instead of the launch's (a test's
    /// "restart": ids minted before it stop matching).
    pub nonce: Option<crate::mcp_serve::document_nonce::LaunchNonce>,
    /// The clock request deadlines are measured on.
    pub clock: Option<Arc<dyn Clock>>,
    /// Demand `Authorization: Bearer <token>` of every request (see
    /// `admission::check_bearer`). `None` keeps the open access.
    pub bearer_token: Option<String>,
    /// Folders file-touching tools may use besides the open documents'
    /// project roots.
    pub extra_roots: Vec<PathBuf>,
}

pub struct McpLiveServer {
    port: u16,
    /// The per-instance admission material the connection threads share; the
    /// server keeps a handle so the bearer requirement and the extra folders
    /// can change while it runs.
    admission: Arc<LiveAdmission>,
    /// The launch nonce its document ids carry.
    nonce: crate::mcp_serve::document_nonce::LaunchNonce,
    /// Per-instance identity token, reported in the live `ping` reply and
    /// written into `~/.openpencil/.op-mcp-port`. Lets the `op` CLI confirm
    /// the server it pings is the exact instance that wrote the discovery
    /// file (defeats stale-file / port-reuse / pid-recycle confusion).
    token: String,
    /// Set by a connection thread when a token-authed `openpencil/shutdown`
    /// arrives; the UI thread polls it (`shutdown_requested`) and exits the
    /// event loop, so `op stop` quits the editor cleanly without a signal.
    quit_flag: Arc<AtomicBool>,
    req_rx: Receiver<UiRequest>,
    stop_tx: Sender<()>,
    /// The connected MCP client's declared identity — `(name, color)`,
    /// captured from `initialize`'s `params.clientInfo.name` the first
    /// time a connection thread sees it (`Arc<Mutex<_>>`: written from
    /// whichever connection thread handles that request, read from the
    /// UI thread in `pump` when tagging a `batch_design` root). A fixed
    /// neutral colour, not the in-app persona pool — an external MCP
    /// client is not one of the app's own named agents.
    client_identity: Arc<Mutex<Option<(String, String)>>>,
    /// The last epoch this server minted for a `batch_design` write, or
    /// `0` before the first one. `pump`'s indicator hook reuses it
    /// (rather than starting a fresh epoch) while its reveal queue is
    /// still draining, so a burst of back-to-back `batch_design` calls
    /// reads as one continuous generation instead of each call clipping
    /// the previous one's tail animation. UI-thread-only (`pump` takes
    /// `&mut self`), so a plain field — no interior mutability needed.
    last_mcp_epoch: u64,
    /// Validated extension evidence waiting for the desktop's asynchronous
    /// provider worker. Kept separate from `EditorState`: this route neither
    /// reads nor mutates the live document.
    pending_design_md: VecDeque<PendingDesignMdRequest>,
    /// The clock and budgets every request's deadline is measured on; the
    /// connection threads hold the same value.
    timing: LiveTiming,
    /// Requests the pump dropped unrun because their caller had stopped
    /// waiting (deadline passed, or already cancelled). UI-thread-only.
    expired_requests: u64,
}

#[derive(Default)]
struct ApplyAck {
    applied: bool,
    /// The apply was refused because its `expected_epoch` no longer names the
    /// document (as opposed to a collaboration refusal or a failed command,
    /// which leave this `false`).
    document_changed: bool,
    /// The addressed document could not be lent to the apply at all.
    unresolved: Option<ResolveError>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct McpPumpOutcome {
    pub repaint: bool,
    pub layout_dirty: bool,
    /// Set when this pump swapped in a whole new document
    /// (`UiRequest::ReplaceDocument` → `EditorState::replace_document`).
    /// `replace_document` resets `document_revision()` back to 0, which can
    /// alias a previously-scanned revision on an unrelated document — the
    /// caller MUST invalidate any revision-keyed cache (e.g. the desktop
    /// host's image-search scan gate) whenever this is `true`, the same way
    /// it already does after a Figma import installs a fresh `EditorState`.
    pub document_replaced: bool,
    /// Set when this pump wrote a new file for the untitled on-screen document
    /// (`create_document`) and marked the document saved. The caller MUST make
    /// that file the document's own — path, tab, window title, recent files —
    /// exactly as a user Save As would.
    pub created_document: Option<std::path::PathBuf>,
}

enum UiRequest {
    /// Every open document, for resolution and for `list_documents`.
    Documents {
        ack: SyncSender<Vec<DocumentListing>>,
    },
    Snapshot {
        document: DocumentId,
        ticket: Arc<Ticket>,
        ack: SyncSender<Result<SnapshotAck, ResolveError>>,
    },
    /// Lightweight read snapshot for `list_pages`. Copying only page metadata
    /// avoids cloning every node in a large live document just to report page
    /// ids, names, and the active index.
    ListPages {
        document: DocumentId,
        ticket: Arc<Ticket>,
        ack: SyncSender<Result<op_mcp::ListPages, ResolveError>>,
    },
    Apply {
        document: DocumentId,
        /// The MCP tool that produced `cmd` — `McpLiveServer::pump` reads
        /// this to gate canvas-generation indicators (frame glow + reveal
        /// sweep) on `"batch_design"` specifically, mirroring how the
        /// in-app design-agent loop only animates that one tool
        /// (`design_agent_tools.rs::should_register_batch_reveals`).
        tool_name: String,
        cmd: EditorCommand,
        /// The document identity the call's snapshot was taken from. The UI
        /// thread refuses the apply when it no longer matches. `None` for a
        /// call that took no snapshot (the lightweight `set_active_page`
        /// path), which has nothing to compare against.
        expected_epoch: Option<DocumentEpoch>,
        /// Carries the caller's deadline. The pump drops the request unapplied
        /// once it has passed, and the caller reports the outcome by which side
        /// won the ticket (see `request_ticket`).
        ticket: Arc<Ticket>,
        ack: SyncSender<ApplyAck>,
    },
    /// Whole-document sync (TS REST `/api/mcp/document` parity), always into
    /// the document on screen. The document is parsed/loaded OFF the UI thread
    /// (so a large document never pins the UI thread, and the document's gate
    /// is held only for the swap); the UI pump just swaps the already-loaded
    /// document into the live `EditorState`. The bool ack is false when the
    /// current collaboration session rejects whole-document replacement.
    ReplaceDocument {
        doc: Box<jian_ops_schema::PenDocument>,
        editor_meta: op_pen_loader::EditorMeta,
        ticket: Arc<Ticket>,
        ack: SyncSender<Result<bool, ResolveError>>,
    },
    /// Editor-only live-sync update. Page switches must not install an
    /// identical whole document as an undoable replacement. The bool ack is
    /// false when the current collaboration session refuses a metadata write.
    UpdateEditorMeta {
        editor_meta: op_pen_loader::EditorMeta,
        ticket: Arc<Ticket>,
        ack: SyncSender<Result<bool, ResolveError>>,
    },
    GenerateDesignMd {
        request: PendingDesignMdRequest,
    },
    /// `create_document`: write the untitled on-screen document to
    /// `<project_root>/designs/<name>.op`. Held to the same epoch check as an
    /// apply, so a document swapped since the call's snapshot is never written.
    CreateDocument {
        document: DocumentId,
        project_root: String,
        name: String,
        expected_epoch: DocumentEpoch,
        ticket: Arc<Ticket>,
        ack: SyncSender<CreateAck>,
    },
    /// `debug_screenshot` against the LIVE canvas — the connection
    /// thread blocks on `ack` while the UI pump renders the active
    /// page / node through the raster export pipeline
    /// (`export::screenshot::capture`). Same pending-intent +
    /// bounded-wait discipline as the chat canvas tools.
    #[cfg(feature = "mcp-debug-tools")]
    Screenshot {
        document: DocumentId,
        spec: crate::export::screenshot::CaptureSpec,
        ack: SyncSender<
            Result<
                Result<crate::export::screenshot::ScreenshotPng, crate::export::ExportError>,
                ResolveError,
            >,
        >,
    },
}

impl UiRequest {
    /// The ticket of a request the connection thread waits on and may give up
    /// on. `None` for requests nobody waits for an answer to.
    fn ticket(&self) -> Option<&Arc<Ticket>> {
        match self {
            UiRequest::Snapshot { ticket, .. }
            | UiRequest::ListPages { ticket, .. }
            | UiRequest::Apply { ticket, .. }
            | UiRequest::ReplaceDocument { ticket, .. }
            | UiRequest::UpdateEditorMeta { ticket, .. }
            | UiRequest::CreateDocument { ticket, .. } => Some(ticket),
            UiRequest::Documents { .. } | UiRequest::GenerateDesignMd { .. } => None,
            #[cfg(feature = "mcp-debug-tools")]
            UiRequest::Screenshot { .. } => None,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            UiRequest::Documents { .. } => "documents",
            UiRequest::Snapshot { .. } => "snapshot",
            UiRequest::ListPages { .. } => "list-pages",
            UiRequest::Apply { .. } => "apply",
            UiRequest::ReplaceDocument { .. } => "document-sync",
            UiRequest::UpdateEditorMeta { .. } => "metadata-sync",
            UiRequest::GenerateDesignMd { .. } => "design-md",
            UiRequest::CreateDocument { .. } => "create-document",
            #[cfg(feature = "mcp-debug-tools")]
            UiRequest::Screenshot { .. } => "screenshot",
        }
    }
}

impl McpLiveServer {
    #[doc(hidden)]
    pub fn start(port: u16) -> Result<Self, McpLiveError> {
        Self::start_with_wake(port, || {})
    }

    pub fn start_with_wake<F>(port: u16, wake_ui: F) -> Result<Self, McpLiveError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self::start_with_options(port, wake_ui, LiveOptions::default())
    }

    /// [`Self::start_with_wake`] with the bearer token, extra folders, nonce
    /// and clock of `options`.
    pub fn start_with_options<F>(
        port: u16,
        wake_ui: F,
        options: LiveOptions,
    ) -> Result<Self, McpLiveError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let timing = match options.clock.clone() {
            Some(clock) => LiveTiming::with_clock(clock),
            None => LiveTiming::system(),
        };
        Self::start_with_timing(port, wake_ui, timing, options)
    }

    /// [`Self::start_with_wake`] on a caller-supplied clock: request deadlines
    /// are measured on it by both the connection threads and the pump.
    #[doc(hidden)]
    pub fn start_with_wake_and_clock<F>(
        port: u16,
        wake_ui: F,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, McpLiveError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self::start_with_options(
            port,
            wake_ui,
            LiveOptions {
                clock: Some(clock),
                ..LiveOptions::default()
            },
        )
    }

    fn start_with_timing<F>(
        port: u16,
        wake_ui: F,
        timing: LiveTiming,
        options: LiveOptions,
    ) -> Result<Self, McpLiveError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let nonce = options
            .nonce
            .unwrap_or_else(crate::mcp_serve::document_nonce::launch_nonce);
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| McpLiveError::Startup(format!("bind 127.0.0.1:{port}: {e}")))?;
        let bound_port = listener
            .local_addr()
            .map_err(|e| McpLiveError::Startup(format!("read bound MCP port: {e}")))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| McpLiveError::Startup(format!("set nonblocking: {e}")))?;
        let (req_tx, req_rx) = mpsc::channel();
        let req_tx = UiSender::new(req_tx, timing.clone()).with_nonce(nonce);
        let (stop_tx, stop_rx) = mpsc::channel();
        let token = make_live_token();
        // The connection threads need both halves of the admission material:
        // the token they must see on every state-touching request, and the
        // port they actually bound (so `Host` can be pinned to it — the
        // DNS-rebinding check). Bundled so `server_loop`'s argument list
        // stays the same width.
        let admission = Arc::new(LiveAdmission::new(token.clone(), bound_port));
        admission.set_bearer(options.bearer_token);
        admission.set_extra_roots(options.extra_roots);
        let server_admission = Arc::clone(&admission);
        let quit_flag = Arc::new(AtomicBool::new(false));
        let server_quit = Arc::clone(&quit_flag);
        let wake_ui: UiWake = Arc::new(wake_ui);
        let client_identity = Arc::new(Mutex::new(None));
        let server_identity = Arc::clone(&client_identity);
        let documents = Arc::new(LiveDocuments::default());
        thread::Builder::new()
            .name("op-mcp-live-http".into())
            .spawn(move || {
                server_loop(
                    listener,
                    req_tx,
                    stop_rx,
                    admission,
                    server_quit,
                    wake_ui,
                    server_identity,
                    documents,
                )
            })
            .map_err(|e| McpLiveError::Startup(format!("spawn MCP live server: {e}")))?;
        eprintln!("openpencil-desktop mcp: listening on 127.0.0.1:{bound_port}/mcp");
        Ok(Self {
            port: bound_port,
            admission: server_admission,
            nonce,
            token,
            quit_flag,
            req_rx,
            stop_tx,
            client_identity,
            last_mcp_epoch: 0,
            pending_design_md: VecDeque::new(),
            timing,
            expired_requests: 0,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The launch nonce this server's document ids carry (`d<N>.<nonce>`).
    pub fn nonce(&self) -> crate::mcp_serve::document_nonce::LaunchNonce {
        self.nonce
    }

    /// Demand `Authorization: Bearer <token>` of every request from now on, or
    /// (`None`) go back to open access. Takes effect on the next request.
    pub fn set_bearer_token(&self, token: Option<String>) {
        self.admission.set_bearer(token);
    }

    /// Whether every request currently needs the bearer token.
    pub fn bearer_required(&self) -> bool {
        self.admission.bearer_required()
    }

    /// Replace the user's extra folders file-touching tools may use.
    pub fn set_extra_roots(&self, roots: Vec<PathBuf>) {
        self.admission.set_extra_roots(roots);
    }

    /// Identity token to publish in the discovery file.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// How many queued requests the pump dropped unrun because their caller
    /// had already stopped waiting. Each one is a change that was NOT applied.
    pub fn expired_requests_dropped(&self) -> u64 {
        self.expired_requests
    }

    /// Whether a token-authed `openpencil/shutdown` was received — the UI
    /// thread should exit the event loop.
    pub fn shutdown_requested(&self) -> bool {
        self.quit_flag.load(Ordering::Acquire)
    }

    /// Consume a pending `openpencil/shutdown`: `true` once per request. The UI
    /// thread uses this so that a quit the user cancels at the unsaved-changes
    /// prompt is dropped instead of being asked again on every loop turn.
    pub fn take_shutdown_request(&self) -> bool {
        self.quit_flag.swap(false, Ordering::AcqRel)
    }

    /// Take the next extension design-analysis request for asynchronous
    /// provider execution by the desktop host. At most one can exist globally,
    /// but this queue-shaped API keeps the UI pump non-blocking.
    pub fn take_pending_design_md_request(&mut self) -> Option<PendingDesignMdRequest> {
        self.pending_design_md.pop_front()
    }

    pub fn stop(&mut self) {
        let _ = self.stop_tx.send(());
    }

    /// Tag a JUST-APPLIED `batch_design` write's genuinely new content
    /// with canvas-generation indicators, so the radar-scan (root frame
    /// glow — `op_editor_ui`'s `canvas_generation_scan` gates purely on
    /// `AgentIndicators.frames` being non-empty) and the per-node reveal
    /// entrance animation activate for MCP-driven generation the same
    /// way they already do for the in-app design-agent loop. A no-op
    /// when nothing new landed (a `batch_design` Direct-op call that
    /// only tweaked an existing node's properties, say).
    fn register_mcp_generation(&mut self, ids_before: &HashSet<String>, state: &EditorState) {
        let now_ms = crate::design_agent_tools::reveal_now_millis();
        let mut saw_new_content = false;
        for node in state.active_children() {
            if !ids_before.contains(node.id_str()) {
                saw_new_content = true;
                break;
            }
        }
        if !saw_new_content {
            // Reveals can still land nested under an EXISTING root (an
            // "add a section to this screen" follow-up) even when no
            // top-level id is new — `register_new_node_reveals` below
            // walks the whole tree, so check that shape too before
            // giving up entirely.
            saw_new_content = state
                .active_children()
                .iter()
                .any(|node| subtree_has_new_id(node, ids_before));
        }
        if !saw_new_content {
            return;
        }
        let epoch = self.resolve_mcp_epoch(now_ms);
        let (name, color) = self
            .client_identity
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_else(|| {
                (
                    MCP_CLIENT_FALLBACK_NAME.to_string(),
                    MCP_CLIENT_COLOR.to_string(),
                )
            });
        // Only a genuinely NEW top-level Frame is a fresh generation
        // root — mirrors `design_loop_indicator.rs::register_new_frames`
        // (Frame-only) and `run_screen_groups.rs::insert_screen_group_roots`
        // (new-vs-existing top-level diff), the two established places
        // this codebase already tags a root frame.
        for node in state.active_children() {
            if matches!(node, PenNode::Frame(_)) && !ids_before.contains(node.id_str()) {
                op_editor_core::agent_indicators::add_frame(epoch, node.id_str(), &color, &name);
            }
        }
        crate::design_agent_tools::register_new_node_reveals(
            ids_before,
            state,
            Some(epoch),
            now_ms,
        );
        self.last_mcp_epoch = epoch;
        // Each `batch_design` call is its own self-contained turn — no
        // idle-timeout bookkeeping needed. `finish_if_epoch` is the
        // established graceful-drain: `run_active` drops immediately but
        // the already-queued reveals keep animating at their own pace
        // (`agent_indicators.rs`'s paint-side maintenance retires the
        // epoch once the queue empties), so this is safe to call even
        // though the call that just landed is still visually settling.
        op_editor_core::agent_indicators::finish_if_epoch(epoch);
    }

    /// Reuse the last MCP epoch while its reveal queue is still
    /// draining (`latest_reveal_end_ms` is the same "is anything still
    /// animating" check the orchestrator's own tree-restructure gate
    /// polls), so a burst of back-to-back `batch_design` calls (a
    /// multi-screen design, one call per screen) reads as one
    /// continuous generation instead of each call clipping the
    /// previous one's tail animation via `begin()`'s "bump epoch +
    /// clear every map" semantics. Starts fresh once the queue empties.
    fn resolve_mcp_epoch(&self, now_ms: u64) -> u64 {
        if self.last_mcp_epoch != 0 {
            if let Some(end_ms) =
                op_editor_core::agent_indicators::latest_reveal_end_ms(self.last_mcp_epoch)
            {
                if now_ms < end_ms {
                    return self.last_mcp_epoch;
                }
            }
        }
        op_editor_core::agent_indicators::begin()
    }
}

/// Whether `node` or any descendant carries an id not in `ids_before` —
/// the "did ANY new content land, even nested under an existing root"
/// check `register_mcp_generation` needs before deciding whether a
/// `batch_design` call is worth animating at all.
fn subtree_has_new_id(node: &PenNode, ids_before: &HashSet<String>) -> bool {
    if !ids_before.contains(node.id_str()) {
        return true;
    }
    if let Some(children) = node.children() {
        return children
            .iter()
            .any(|child| subtree_has_new_id(child, ids_before));
    }
    false
}

impl Drop for McpLiveServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn command_invalidates_layout(cmd: &EditorCommand) -> bool {
    match cmd {
        EditorCommand::ClearSelection
        | EditorCommand::SetSelection { .. }
        | EditorCommand::SetSelectionSet { .. }
        | EditorCommand::ToggleNodeSelection { .. }
        | EditorCommand::SetViewport { .. }
        | EditorCommand::SetActiveTool { .. }
        | EditorCommand::CopySelected => false,
        EditorCommand::SetNodeFlag { flag, .. } => {
            !matches!(flag, op_editor_core::NodeFlag::Collapsed)
        }
        _ => true,
    }
}

mod addressing_schema;
mod admission;
mod connection;
mod create_document_route;
mod design_md_output;
pub(crate) mod design_md_route;
mod doc_sync;
mod document_epoch;
mod document_gates;
mod document_target;
mod live_call;
mod live_timing;
mod operation_log;
mod path_policy;
mod pump;
mod request_ticket;
mod resolver;
/// `pub(crate)` for two consts only: the shared HTTP parser
/// (`mcp_serve::read_http_request`) has to recognise this route's path to
/// apply its smaller body cap before reading a body.
pub(crate) mod snapshot_ingest;
#[cfg(test)]
mod test_support;
mod ui_requests;

use admission::*;
use connection::*;
use create_document_route::CreateAck;
use doc_sync::*;
use document_epoch::*;
use document_gates::*;
use document_target::active_document;
use live_timing::{LiveTiming, UiSender};
use request_ticket::Ticket;
use ui_requests::*;

#[cfg(test)]
#[path = "mcp_live/bearer_tests.rs"]
mod bearer_tests;
#[cfg(test)]
#[path = "mcp_live/document_addressing_tests.rs"]
mod document_addressing_tests;
#[cfg(test)]
#[path = "mcp_live/document_nonce_tests.rs"]
mod document_nonce_tests;
#[cfg(test)]
#[path = "mcp_live/file_confinement_tests.rs"]
mod file_confinement_tests;
#[cfg(test)]
#[path = "mcp_live/request_deadline_tests.rs"]
mod request_deadline_tests;
#[cfg(test)]
#[path = "mcp_live_tests.rs"]
mod tests;
