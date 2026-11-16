//! The bearer token in terminal-CLI MCP configs (`mcp_require_token`).
//!
//! When the live server requires `Authorization: Bearer <token>`, a CLI can
//! only keep working if its MCP config carries that header on every request.
//! Most CLIs' configs have a static-header field; a few do not (or this change
//! could not confirm one), and for those the entry is left as it was and the
//! CLI stops working while the requirement is on.
//!
//! # Which CLIs carry the header
//!
//! Confirmed against each CLI's own documentation (read 2026-11):
//!
//! | CLI | config field written | documentation |
//! | --- | --- | --- |
//! | Claude Code | `headers` | code.claude.com/docs/en/mcp |
//! | Codex | `http_headers` (TOML inline table) | developers.openai.com/codex/mcp |
//! | OpenCode | `headers` | opencode.ai/docs/mcp-servers |
//! | Kiro | `headers` | kiro.dev/docs/mcp/configuration |
//! | GitHub Copilot | `headers` | docs.github.com Copilot CLI "add MCP servers" |
//! | Antigravity | `headers` | antigravity.google/docs/mcp ("Custom headers") |
//! | Gemini CLI | `headers` | geminicli.com/docs/tools/mcp-server |
//! | Qwen Code | `headers` | qwenlm.github.io/qwen-code-docs (MCP) |
//! | Cursor | `headers` | cursor.com/docs/context/mcp |
//! | Kimi | `headers` | moonshotai.github.io/kimi-cli (MCP) |
//!
//! NOT written, so these stop working while the requirement is on: Grok Build,
//! ZCode and DeepSeek Harness. Their documentation was not found, and a
//! `headers` key in a format that does not define it can make a strict config
//! parser reject the whole file. [`supports_bearer_header`] is the one place
//! that list lives.

use std::path::Path;

use op_editor_core::agent_settings::McpCli;
use serde_json::{json, Value};

/// Where a CLI should be pointed, and with what credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LiveEndpoint<'a> {
    pub(crate) port: u16,
    /// The six-tool lean profile (`/mcp/lean`) instead of the full catalog.
    pub(crate) lean: bool,
    /// The bearer token to write as an `Authorization` header, for the CLIs
    /// that [`supports_bearer_header`]. `None` writes no header.
    pub(crate) bearer: Option<&'a str>,
}

impl LiveEndpoint<'_> {
    /// An endpoint with no credential: the default open access.
    #[cfg(test)]
    pub(crate) fn open(port: u16, lean: bool) -> LiveEndpoint<'static> {
        LiveEndpoint {
            port,
            lean,
            bearer: None,
        }
    }
}

/// Whether `cli`'s MCP config can carry a static `Authorization` header (see
/// the module table). False means the entry is left without a credential.
pub(crate) fn supports_bearer_header(cli: McpCli) -> bool {
    !matches!(cli, McpCli::GrokBuild | McpCli::ZCode | McpCli::Dsh)
}

/// The token to write for `cli`: `bearer` when the CLI can carry it.
pub(crate) fn token_for(cli: McpCli, bearer: Option<&str>) -> Option<&str> {
    bearer.filter(|_| supports_bearer_header(cli))
}

/// `{"Authorization": "Bearer <token>"}`.
pub(crate) fn authorization_headers(token: &str) -> Value {
    json!({ "Authorization": format!("Bearer {token}") })
}

/// `server` with the `headers` object added when `token` is `Some`.
pub(crate) fn with_headers(mut server: Value, token: Option<&str>) -> Value {
    if let (Some(token), Some(object)) = (token, server.as_object_mut()) {
        object.insert("headers".into(), authorization_headers(token));
    }
    server
}

/// The Codex line for `token`:
/// `http_headers = { Authorization = "Bearer <token>" }` (TOML inline table;
/// a hex token needs no escaping, but it goes through the same escaper as the
/// URL anyway).
pub(crate) fn codex_headers_line(token: &str, escape: fn(&str) -> String) -> String {
    format!(
        "http_headers = {{ Authorization = \"Bearer {}\" }}\n",
        escape(token)
    )
}

/// A config file that holds a token must not be readable by other accounts. A
/// file that already existed keeps the permissions its owner chose; one this
/// write created is restricted to the owner.
#[cfg(unix)]
pub(crate) fn restrict_new_file(path: &Path, existed: bool) {
    use std::os::unix::fs::PermissionsExt;
    if !existed {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

#[cfg(not(unix))]
pub(crate) fn restrict_new_file(_path: &Path, _existed: bool) {}
