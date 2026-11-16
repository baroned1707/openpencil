//! `create_document` on the live desktop server: give the untitled on-screen
//! document its first file.
//!
//! Two halves, like every live write. The connection thread validates the
//! arguments (read-only filesystem checks, so a bad call never wakes the UI
//! thread), learns the document's epoch from a snapshot and hands the request
//! to the UI thread. The UI thread then re-checks everything against the live
//! editor, writes the file and marks the document saved, all in one step so no
//! other request can land between the write and the "saved" mark. The desktop
//! host finishes the binding (path, tab, title, recent files) from the
//! `created_document` it finds in the pump outcome.

use std::path::PathBuf;

use op_mcp::{RequestId, ToolResponse};

use super::*;
use crate::mcp_serve::create_document::{
    self, CreateDocumentError, TOOL_NAME as CREATE_DOCUMENT_TOOL,
};
use crate::mcp_serve::tool_profile::McpAccessProfile;

/// The file the UI thread wrote, as reported to the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreatedDocument {
    pub(super) file_path: PathBuf,
    pub(super) project_root: PathBuf,
    pub(super) name: String,
}

/// The UI thread's answer to a create request.
pub(super) enum CreateAck {
    Done(Result<CreatedDocument, CreateDocumentError>),
    /// The on-screen document was replaced since the call's snapshot.
    DocumentChanged,
}

/// Answer a `create_document` call, or `None` when `body` is some other call
/// (or one the session's profile refuses, which the generic dispatch reports).
pub(super) fn serve(
    body: &str,
    profile: McpAccessProfile,
    open_paths: &OpenDocumentPaths,
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
) -> Result<Option<String>, McpLiveError> {
    let Some(call) = op_mcp::parse_tool_call(body.trim()) else {
        return Ok(None);
    };
    if call.tool != CREATE_DOCUMENT_TOOL
        || profile.refuse_call(&call.tool, &call.arguments).is_some()
    {
        return Ok(None);
    }
    let refuse = |id: RequestId, error: CreateDocumentError| {
        Ok(Some(create_document::error_response(id, &error)))
    };
    if let Some(path) = open_paths.active() {
        return refuse(call.id, CreateDocumentError::AlreadySaved { path });
    }
    let validated = create_document::extract_args(&call.arguments).and_then(|(root, name)| {
        create_document::resolve_target(
            &root,
            &name,
            crate::project_root::canonical_home().as_deref(),
        )
        .map(|_| (root, name))
    });
    let (project_root, name) = match validated {
        Ok(args) => args,
        Err(error) => return refuse(call.id, error),
    };
    // Only the epoch is wanted; the clone of the document is dropped at once.
    let (_snapshot, epoch) = request_snapshot(req_tx, wake_ui)?;
    match request_create(req_tx, wake_ui, project_root, name, epoch)? {
        CreateAck::DocumentChanged => Ok(document_changed_response(body)),
        CreateAck::Done(Err(error)) => refuse(call.id, error),
        CreateAck::Done(Ok(created)) => Ok(Some(success_response(call.id, &created))),
    }
}

fn success_response(id: RequestId, created: &CreatedDocument) -> String {
    let result = serde_json::json!({
        "ok": true,
        "filePath": created.file_path.to_string_lossy(),
        "projectRoot": created.project_root.to_string_lossy(),
        "name": created.name,
    });
    op_mcp::tool_response_to_json(&ToolResponse::Ok {
        id,
        result: Default::default(),
        command: None,
        json: Some(result.to_string()),
        image: None,
    })
}

/// Ask the UI thread to create the document's file.
pub(super) fn request_create(
    req_tx: &Sender<UiRequest>,
    wake_ui: &UiWake,
    project_root: String,
    name: String,
    expected_epoch: DocumentEpoch,
) -> Result<CreateAck, McpLiveError> {
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    req_tx
        .send(UiRequest::CreateDocument {
            project_root,
            name,
            expected_epoch,
            ack: ack_tx,
        })
        .map_err(|_| {
            McpLiveError::UiThread(
                "UI thread is not accepting MCP create-document requests".to_string(),
            )
        })?;
    wake_ui();
    recv_with_timeout(ack_rx.recv_timeout(UI_ACK_TIMEOUT), "create-document")
}

/// UI-thread half. Re-checks against the live editor, writes the file and
/// marks the document saved, then makes the new path visible to the
/// connection threads BEFORE the ack: the stateful lock is released as soon as
/// the reply is written, so a follow-up `filePath` call naming the new file
/// must already be refused as an open document.
pub(super) fn create_on_ui_thread(
    state: &mut EditorState,
    open_paths: &OpenDocumentPaths,
    project_root: &str,
    name: &str,
) -> Result<CreatedDocument, CreateDocumentError> {
    if let Some(path) = open_paths.active() {
        return Err(CreateDocumentError::AlreadySaved { path });
    }
    if let Err(reason) = CollabGatePolicy::from(&state.editor_ui.collab)
        .check(CollabGateAction::ReplaceDocument, CollabEditSource::Mcp)
    {
        state.editor_ui.collab.set_notice(
            reason.notice_kind(),
            crate::design_agent_tools::reveal_now_millis(),
        );
        return Err(CreateDocumentError::CollaborationActive);
    }
    let target = create_document::resolve_target(
        project_root,
        name,
        crate::project_root::canonical_home().as_deref(),
    )?;
    create_document::write_new_document(state, &target)?;
    state.mark_saved_revision();
    crate::doc_io::set_file_name_display(state, Some(&target.file_path));
    open_paths.bind(&target.file_path);
    Ok(CreatedDocument {
        file_path: target.file_path,
        project_root: target.project_root,
        name: target.name,
    })
}

#[cfg(test)]
#[path = "create_document_route_tests.rs"]
mod tests;
