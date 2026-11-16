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
//! `created` entry (document id and file) it finds in the pump outcome.

use std::path::{Path, PathBuf};

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
    /// The document was replaced since the call's snapshot.
    DocumentChanged,
    /// The addressed document could not be lent to the UI thread at all.
    Unresolved(ResolveError),
}

/// Answer a `create_document` call for `document`, or `None` when `body` is
/// some other call (or one the session's profile refuses, which the caller
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
    if call.tool != CREATE_DOCUMENT_TOOL
        || profile.refuse_call(&call.tool, &call.arguments).is_some()
    {
        return Ok(None);
    }
    let refuse = |id: RequestId, error: CreateDocumentError| {
        Ok(Some(create_document::error_response(id, &error)))
    };
    let bound_path = request_documents(req_tx, wake_ui)?
        .into_iter()
        .find(|listing| listing.id == document)
        .and_then(|listing| listing.path);
    if let Some(path) = bound_path {
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
    let (_snapshot, epoch) = request_snapshot(req_tx, wake_ui, document)?;
    match request_create(req_tx, wake_ui, document, project_root, name, epoch)? {
        CreateAck::DocumentChanged => Ok(document_changed_response(body)),
        CreateAck::Unresolved(reason) => Err(resolve_failure(req_tx, wake_ui, document, reason)),
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
    req_tx: &UiSender,
    wake_ui: &UiWake,
    document: DocumentId,
    project_root: String,
    name: String,
    expected_epoch: DocumentEpoch,
) -> Result<CreateAck, McpLiveError> {
    round_trip(
        req_tx,
        wake_ui,
        "create-document",
        Effect::Mutating,
        |ticket, ack| UiRequest::CreateDocument {
            document,
            project_root,
            name,
            expected_epoch,
            ticket,
            ack,
        },
    )
}

/// UI-thread half. Re-checks against the live editor and writes the file and
/// marks the document saved. `bound_path` is the file the document already
/// has (`None` while untitled). The pump stops draining after a success so the
/// host binds the new path to the document before any later request reads the
/// open files.
pub(super) fn create_on_ui_thread(
    state: &mut EditorState,
    bound_path: Option<&Path>,
    paths_open_elsewhere: &[PathBuf],
    project_root: &str,
    name: &str,
) -> Result<CreatedDocument, CreateDocumentError> {
    if let Some(path) = bound_path {
        return Err(CreateDocumentError::AlreadySaved {
            path: path.to_path_buf(),
        });
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
    if is_bound_elsewhere(&target.file_path, paths_open_elsewhere) {
        return Err(CreateDocumentError::OpenInAnotherDocument {
            path: target.file_path,
        });
    }
    create_document::write_new_document(state, &target)?;
    state.mark_saved_revision();
    crate::doc_io::set_file_name_display(state, Some(&target.file_path));
    Ok(CreatedDocument {
        file_path: target.file_path,
        project_root: target.project_root,
        name: target.name,
    })
}

/// Whether `target`, a file that may not exist yet, is the file of one of
/// `open_paths`. Compared on resolved paths so a symlink or a `..` hop does not
/// hide it.
fn is_bound_elsewhere(target: &Path, open_paths: &[PathBuf]) -> bool {
    let resolve_new = |path: &Path| {
        let parent = path.parent().and_then(|p| std::fs::canonicalize(p).ok());
        match (parent, path.file_name()) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    };
    let wanted = resolve_new(target);
    open_paths
        .iter()
        .any(|open| resolve_new(open) == wanted || open == target)
}

#[cfg(test)]
#[path = "create_document_route_tests.rs"]
mod tests;
