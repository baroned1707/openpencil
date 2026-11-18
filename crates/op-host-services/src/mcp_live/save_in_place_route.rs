//! `save_document` WITHOUT a `filePath` on the live desktop server: save the
//! addressed document to its own file (the Cmd+S equivalent).
//!
//! With a `filePath` the tool writes a COPY and refuses a file an editor tab
//! holds (`path_policy`). This route claims two calls: the one that names no
//! file at all ([`serve`]), and the one whose `filePath` is the document's OWN
//! file ([`serve_own_path`], tried before the path policy, which would refuse
//! that path as "open in an editor tab"). The UI thread, which knows the
//! document's bound path, decides whether a claimed path is its own
//! (`path_policy::names_open_file`); when it is not, the request answers
//! [`SaveAck::NotOwnPath`] without touching the document and the call goes on
//! to the path policy and the copy writer exactly as if there had been no
//! claim. Once the path IS the document's, every refusal of the save stands
//! and none falls through to the copy writer.
//!
//! The connection thread sends one ticketed request; the UI
//! thread runs [`save_in_place`] on the live document (path, collaboration,
//! disk-fingerprint checks, atomic write, "saved" mark in one step, so no edit
//! can land between the write and the mark) and records the follow-up the host
//! must apply (`McpPumpOutcome::saved_in_place`). A retried `operationId`
//! replays the first answer through the normal operation log.

use op_mcp::{RequestId, ToolErrorCode, ToolResponse};

use super::*;
use crate::mcp_serve::tool_profile::McpAccessProfile;
use crate::save_in_place::{
    is_in_place_save_call, own_path_claim, result_json, save_in_place_unless_busy,
    SaveInPlaceError, SavedInPlace,
};

/// The UI thread's answer.
pub(super) enum SaveAck {
    Done(Result<SavedInPlace, SaveInPlaceError>),
    /// The call's `filePath` is not the document's own file (or the document
    /// has none yet): nothing was saved.
    NotOwnPath,
    /// The addressed document could not be lent to the UI thread at all.
    Unresolved(ResolveError),
}

/// Answer a filePath-less `save_document` for `document`, or `None` when `body`
/// is some other call (or one the session's profile refuses, which the caller
/// reports).
pub(super) fn serve(
    body: &str,
    profile: McpAccessProfile,
    document: DocumentId,
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<Option<String>, McpLiveError> {
    let Some(call) = op_mcp::parse_tool_call(body.trim()) else {
        return Ok(None);
    };
    if !is_in_place_save_call(&call) || profile.refuse_call(&call.tool, &call.arguments).is_some() {
        return Ok(None);
    }
    answer(call.id, document, None, req_tx, wake_ui)
}

/// Answer a `save_document` whose `filePath` is `document`'s own file, or
/// `None` when `body` is some other call, the path is not that file (the call
/// is then an ordinary copy) or the session's profile refuses the call.
pub(super) fn serve_own_path(
    body: &str,
    profile: McpAccessProfile,
    document: DocumentId,
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<Option<String>, McpLiveError> {
    let Some(call) = op_mcp::parse_tool_call(body.trim()) else {
        return Ok(None);
    };
    let Some(claimed) = own_path_claim(&call) else {
        return Ok(None);
    };
    if profile.refuse_call(&call.tool, &call.arguments).is_some() {
        return Ok(None);
    }
    let claimed = claimed.to_string();
    answer(call.id, document, Some(claimed), req_tx, wake_ui)
}

/// Send the save to the UI thread and turn its answer into the tool response.
fn answer(
    id: RequestId,
    document: DocumentId,
    claimed_path: Option<String>,
    req_tx: &UiSender,
    wake_ui: &UiWake,
) -> Result<Option<String>, McpLiveError> {
    let ack = round_trip(
        req_tx,
        wake_ui,
        "save-document",
        Effect::Mutating,
        |ticket, ack| UiRequest::SaveInPlace {
            document,
            claimed_path,
            ticket,
            ack,
        },
    )?;
    match ack {
        SaveAck::NotOwnPath => Ok(None),
        SaveAck::Unresolved(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
        SaveAck::Done(Err(error)) => Ok(Some(error_response(id, &error))),
        SaveAck::Done(Ok(saved)) => Ok(Some(success_response(
            id,
            &saved,
            &crate::mcp_serve::document_address::format_spoken_id(document, Some(req_tx.nonce())),
        ))),
    }
}

/// UI-thread half, before the document is lent: whether a claimed path is not
/// the document's own file. An untitled document has no file to be named.
pub(super) fn is_not_own_path(target: &resolver::InPlaceSaveTarget, claimed: Option<&str>) -> bool {
    claimed.is_some_and(|claimed| {
        !target
            .path
            .as_deref()
            .is_some_and(|own| path_policy::names_open_file(claimed, own))
    })
}

/// UI-thread half: the save itself.
pub(super) fn save_on_ui_thread(
    state: &mut EditorState,
    target: &resolver::InPlaceSaveTarget,
) -> Result<SavedInPlace, SaveInPlaceError> {
    save_in_place_unless_busy(
        state,
        target.path.as_deref(),
        target.baseline.as_ref(),
        target.save_in_flight,
    )
}

fn error_response(id: RequestId, error: &SaveInPlaceError) -> String {
    let code = match error {
        SaveInPlaceError::Untitled => ToolErrorCode::InvalidArgument,
        _ => ToolErrorCode::ToolFailed,
    };
    op_mcp::tool_response_to_json(&ToolResponse::Err {
        id,
        code,
        message: error.to_string(),
    })
}

fn success_response(id: RequestId, saved: &SavedInPlace, document_id: &str) -> String {
    op_mcp::tool_response_to_json(&ToolResponse::Ok {
        id,
        result: Default::default(),
        command: None,
        json: Some(result_json(saved, document_id)),
        image: None,
    })
}
