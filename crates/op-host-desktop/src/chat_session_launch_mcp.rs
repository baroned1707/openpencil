//! Which MCP endpoint a Claude Code chat turn is bound to.
//!
//! A turn started in document A must edit A even after the user switches to
//! B, and a multi-document session refuses an unbound `/mcp` call. The launch
//! context therefore carries the live server's facts, and the provider built
//! for the turn gets `/mcp/d/<id>` (or `/mcp/d/<id>/lean`) for the launching
//! document. The endpoint is resolved once, when the provider is built, and
//! never again.

use op_editor_core::AgentSettings;
use op_host_services::chat_claude::McpEndpoint;

use super::providers::ChatLaunchContext;

impl ChatLaunchContext {
    /// The same context carrying the live MCP server's facts: the port only
    /// while the server runs, the lean-profile choice and whether the server
    /// requires a bearer token. The one place these are read from settings.
    pub fn with_mcp_settings(mut self, settings: &AgentSettings) -> Self {
        self.mcp_port = settings
            .mcp_server
            .running
            .then_some(settings.mcp_server.port);
        self.mcp_lean = settings.mcp_lean_profile;
        self.mcp_require_token = settings.mcp_require_token;
        self
    }

    /// The endpoint for this context's document, or `None` (today's unbound
    /// behaviour) without a document or a running server. The token file is
    /// read here and only when the setting requires a token.
    pub(crate) fn claude_mcp_endpoint(&self) -> Option<McpEndpoint> {
        self.claude_mcp_endpoint_with(crate::mcp_port_file::bearer_token)
    }

    /// [`Self::claude_mcp_endpoint`] with the token source injected.
    ///
    /// A required token that cannot be read yields `None`: the turn then falls
    /// back to the global entry, which carries its own header, instead of
    /// hitting a bound endpoint that would answer 401.
    pub(crate) fn claude_mcp_endpoint_with(
        &self,
        read_token: impl FnOnce() -> Option<std::io::Result<String>>,
    ) -> Option<McpEndpoint> {
        let (port, document) = (self.mcp_port?, self.document?);
        let url = op_host_services::mcp_live::mcp_url_for_document(port, document, self.mcp_lean);
        let bearer = if self.mcp_require_token {
            match read_token() {
                Some(Ok(token)) => Some(token),
                Some(Err(error)) => {
                    eprintln!(
                        "openpencil-desktop claude chat: cannot read the MCP token ({error}); \
                         the turn stays unbound"
                    );
                    return None;
                }
                None => return None,
            }
        } else {
            None
        };
        Some(McpEndpoint::new(url, bearer))
    }
}

// Test seam: what the Claude arm of `provider_for_agent` last bound.
#[cfg(test)]
thread_local! {
    pub(super) static LAST_CLAUDE_BINDING: std::cell::RefCell<Option<Option<McpEndpoint>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn record_claude_binding(provider: &op_host_services::chat_claude::ClaudeCodeProvider) {
    let bound = provider.document_mcp().cloned();
    LAST_CLAUDE_BINDING.with(|slot| *slot.borrow_mut() = Some(bound));
}
