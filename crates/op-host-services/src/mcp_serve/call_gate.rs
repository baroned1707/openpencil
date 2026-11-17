//! The checks one tool call must pass BEFORE its registry is built.
//!
//! Running ahead of the tool's own argument parsing is the point: a denied
//! tool never sees the path it was asked to open, and a node-writing call
//! that names a node the document does not have is answered with the id to
//! fix instead of the host's rejection dump.

use op_editor_core::EditorState;
use op_mcp::{ToolCall, ToolErrorCode};

use super::tool_profile::McpAccessProfile;

/// Why `call` must not run, as the error code and text the client reads.
pub(super) fn refusal_before_dispatch(
    state: &EditorState,
    profile: McpAccessProfile,
    call: &ToolCall,
) -> Option<(ToolErrorCode, String)> {
    if let Some(refusal) = profile.refuse_call(&call.tool, &call.arguments) {
        return Some((ToolErrorCode::ToolFailed, refusal.message(&call.tool)));
    }
    op_mcp::node_id_check::unknown_node_refusal(state, call)
        .map(|message| (ToolErrorCode::InvalidArgument, message))
}
