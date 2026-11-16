//! One state-touching MCP request on the live server, from "which document is
//! this about?" to the response.
//!
//! Split out of `connection.rs`. The steps, in order:
//!
//! 1. take the addressing arguments (`documentId`, `operationId`) off the body
//!    so no tool ever sees them, and check the optional `Mcp-Param-DocumentId`
//!    header against the body;
//! 2. refuse a tool the endpoint's profile does not offer (before anything
//!    reaches the UI thread);
//! 3. answer `list_documents` and `filePath` calls, which belong to no open
//!    document;
//! 4. resolve the document (`document_target`), take ITS gate, replay a
//!    retried `operationId`, then run the call against that document;
//! 5. remember the response of a successful write under its `operationId`.

use std::time::Instant;

use op_mcp::{ToolCall, ToolResponse};

use super::operation_log::{replay_response, CallFingerprint, Lookup};
use super::*;
use crate::mcp_serve::document_address::{
    check_document_header, header_mismatch_body, split_address, tool_error_response, CallAddress,
    LiveRoute,
};
use crate::mcp_serve::document_listing::listings_json;
use crate::mcp_serve::tool_catalog::McpToolCatalog;
use crate::mcp_serve::tool_profile::{tool_writes, McpAccessProfile};
use document_target::{resolve_document, Addressing};

/// What every step of a call shares.
struct CallContext<'a> {
    req_tx: &'a Sender<UiRequest>,
    wake_ui: &'a UiWake,
    documents: &'a LiveDocuments,
    profile: McpAccessProfile,
    route: LiveRoute,
    /// The body with `documentId` / `operationId` already removed.
    body: &'a str,
    address: &'a CallAddress,
    /// The request parsed as a tool call; `None` for `tools/list` and for a
    /// body the parser refused.
    tool_call: Option<&'a ToolCall>,
    /// The request is a `tools/list`: a catalog, not a call about a document.
    lists_tools: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn serve_call<S: std::io::Write>(
    stream: &mut S,
    req: &crate::mcp_serve::HttpRequest,
    route: LiveRoute,
    profile: McpAccessProfile,
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    documents: &LiveDocuments,
) -> Result<(), McpLiveError> {
    let (address, body) = split_address(&req.body);
    if let Err(mismatch) = check_document_header(
        req.mcp_param_document_id.as_deref(),
        address.document_id.as_deref(),
    ) {
        return write_http(
            stream,
            "400 Bad Request",
            &header_mismatch_body(&req.body, &mismatch),
        );
    }
    let body: &str = &body;
    let parsed = op_mcp::parse_tool_call(body.trim());
    // MCP method names contain a `/` (`tools/list`); tool names never do, and
    // the legacy direct shape parses a method as if it were a tool.
    let tool_call = parsed.as_ref().filter(|call| !call.tool.contains('/'));
    if let Some(call) = tool_call {
        if let Some(refusal) = profile.refuse_call(&call.tool, &call.arguments) {
            let message = refusal.message(&call.tool);
            if let Some(response) = tool_error_response(body, message) {
                return write_json_rpc_response(stream, &response);
            }
        }
    }
    let context = CallContext {
        req_tx,
        wake_ui,
        documents,
        profile,
        route,
        body,
        address: &address,
        tool_call,
        lists_tools: parsed
            .as_ref()
            .is_some_and(|call| call.tool == "tools/list"),
    };
    let response = match run_call(&context) {
        Ok(response) => response,
        Err(error) if error.is_tool_error() => {
            tool_error_response(body, error.to_string()).ok_or(error)?
        }
        Err(error) => return Err(error),
    };
    write_json_rpc_response(stream, &response)
}

fn run_call(context: &CallContext) -> Result<String, McpLiveError> {
    if let Some(call) = context.tool_call {
        if call.tool == "list_documents" {
            let open = request_documents(context.req_tx, context.wake_ui)?;
            return Ok(op_mcp::tool_response_to_json(&ToolResponse::Ok {
                id: call.id.clone(),
                result: Default::default(),
                command: None,
                json: Some(listings_json(&open)),
                image: None,
            }));
        }
        if names_a_file(call) {
            // A file on disk is read, edited and written back: serialize those
            // calls (they belong to no open document) and refuse a file the
            // editor has open.
            let _files = context.documents.lock_files();
            let open_paths: Vec<std::path::PathBuf> =
                request_documents(context.req_tx, context.wake_ui)?
                    .into_iter()
                    .filter_map(|listing| listing.path)
                    .collect();
            if let Some(response) = crate::mcp_serve::file_path::process_message_for_file_path_arg(
                None,
                &open_paths,
                context.body,
                context.profile,
            )? {
                return Ok(response);
            }
        }
    }
    let addressing = if context.tool_call.is_some() {
        Addressing::Call
    } else {
        Addressing::Catalog
    };
    let document = resolve_document(
        context.route.document,
        context.address.document_id.as_deref(),
        addressing,
        context.req_tx,
        context.wake_ui,
    )?;
    let result = run_in_document(context, document);
    if matches!(result, Err(McpLiveError::UnknownDocument { .. })) {
        context.documents.forget(document);
    }
    result
}

/// The call runs under its document's gate: other documents' calls proceed,
/// this document's wait.
fn run_in_document(context: &CallContext, document: DocumentId) -> Result<String, McpLiveError> {
    let gate = context.documents.gate(document);
    let mut log = lock_gate(&gate);
    let operation = context
        .tool_call
        .filter(|call| tool_writes(&call.tool))
        .zip(context.address.operation_id.as_deref())
        .map(|(call, operation_id)| {
            (
                call,
                operation_id,
                CallFingerprint::of(&call.tool, &call.arguments),
            )
        });
    if let Some((call, operation_id, fingerprint)) = &operation {
        match log.lookup(operation_id, *fingerprint, Instant::now()) {
            Lookup::Replay(stored) => return Ok(replay_response(&stored, &call.id)),
            Lookup::Conflict => {
                return Err(McpLiveError::OperationIdConflict {
                    operation_id: operation_id.to_string(),
                })
            }
            Lookup::Miss => {}
        }
    }
    let mut response = dispatch(context, document)?;
    if let Some((_, operation_id, fingerprint)) = &operation {
        if crate::mcp_serve::mcp_response_succeeded(&response) {
            log.record(operation_id, *fingerprint, response.clone(), Instant::now());
        }
    }
    if context.route.document.is_none() && context.lists_tools {
        response = addressing_schema::annotate_tools_list(&response);
    }
    Ok(response)
}

/// Run the call against `document`, holding its gate.
fn dispatch(context: &CallContext, document: DocumentId) -> Result<String, McpLiveError> {
    let CallContext {
        req_tx,
        wake_ui,
        profile,
        body,
        ..
    } = *context;
    let full_catalog = context.route.catalog == McpToolCatalog::Full;
    // `debug_screenshot` against the LIVE canvas: route through the raster
    // export pipeline on the UI thread instead of the generic dispatch (whose
    // headless tool can only report no-live-canvas). Gate-closed
    // (`OPENPENCIL_DEBUG_TOOLS` unset) falls through to the generic dispatch,
    // which reports UnknownTool exactly like the headless registry that never
    // registered the tool.
    #[cfg(feature = "mcp-debug-tools")]
    if let Some(response) = full_catalog
        .then(|| {
            screenshot::maybe_serve(body, op_mcp::debug_tools_enabled(), |shot_req| {
                request_screenshot(req_tx, wake_ui, document, shot_req)
            })
        })
        .flatten()
    {
        return Ok(response);
    }
    // These tools need either no editor snapshot (`set_active_page`) or only
    // page metadata (`list_pages`). Keep them on the normal MCP
    // parser/registry/serializer path, but do not deep-clone the live
    // document. Neither tool is in the lean catalog, so a lean request skips
    // the fast path.
    if full_catalog {
        if let Some(response) = process_lightweight_live_tool(body, document, req_tx, wake_ui)? {
            return Ok(response);
        }
    }
    // `create_document` writes a file for the untitled document and needs the
    // host's view of it, so it has its own two-step route. Under a profile
    // that refuses it (the lean catalog) the generic dispatch reports the
    // refusal.
    if let Some(response) = create_document_route::serve(body, profile, document, req_tx, wake_ui)?
    {
        return Ok(response);
    }
    let (mut state, epoch) = match request_snapshot(req_tx, wake_ui, document) {
        Ok(snapshot) => snapshot,
        // A catalog must stay readable when the document its endpoint is bound
        // to is parked or closed (the user switched tabs): list the tools from
        // an empty document, which still yields the static catalog and the
        // built-in kits.
        Err(McpLiveError::DocumentNotOnScreen { .. } | McpLiveError::UnknownDocument { .. })
            if context.lists_tools =>
        {
            let state = EditorState::new();
            let epoch = DocumentEpoch::of(0, &state);
            (state, epoch)
        }
        Err(error) => return Err(error),
    };
    let mut failure: Option<McpLiveError> = None;
    let response = crate::mcp_serve::process_message_with_applier_profiled(
        &mut state,
        body,
        profile,
        |tool_name, local_state, cmd| match request_apply(
            req_tx,
            wake_ui,
            document,
            tool_name.to_string(),
            cmd.clone(),
            Some(epoch),
        ) {
            Ok(ack) => {
                if ack.applied {
                    let _ = local_state.apply(cmd.clone());
                }
                ack.applied
            }
            Err(error) if error.is_tool_error() => {
                failure = Some(error);
                false
            }
            Err(error) => {
                eprintln!("openpencil-desktop mcp: apply failed: {error}");
                false
            }
        },
    )?
    .unwrap_or_default();
    // The dispatcher only knows the applier said `false`; give the client the
    // real reason as a tool error instead of "host rejected command".
    match failure {
        Some(error) => Ok(tool_error_response(body, error.to_string()).unwrap_or(response)),
        None => Ok(response),
    }
}

/// Whether the call targets a `.op` file on disk (`filePath`, or
/// `save_document`'s `sourceFilePath`).
fn names_a_file(call: &ToolCall) -> bool {
    ["filePath", "sourceFilePath"].iter().any(|key| {
        call.arguments
            .get(*key)
            .is_some_and(|value| !value.trim().is_empty())
    })
}
