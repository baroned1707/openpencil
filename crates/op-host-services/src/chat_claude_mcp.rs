//! Binds one Claude Code chat turn to ITS document's MCP endpoint.
//!
//! The `claude --print` subprocess otherwise reaches the canvas only through
//! the user's global `openpencil` entry (`/mcp`, no document), which a
//! multi-document session refuses with a listing. The chat launcher hands the
//! provider the launching document's endpoint (`/mcp/d/<id>`); this module
//! turns it into a per-turn `--mcp-config` through the SDK options.
//!
//! # Why a distinct server name
//!
//! `claude --help` documents `--mcp-config <configs...>` as "Load MCP servers
//! from JSON files or strings" and `--strict-mcp-config` as "Only use MCP
//! servers from --mcp-config, ignoring all other MCP configurations". Nothing
//! there says which entry wins when `--mcp-config` and the user's config both
//! define the same name, so the bound server is registered as
//! [`BOUND_SERVER_NAME`] and cannot collide with the global `openpencil`.
//! `--strict-mcp-config` is never passed: the user's other servers keep working.
//! The turn's system prompt names the bound tools so the agent prefers them.
//!
//! # The token
//!
//! With `mcp_require_token` on, the endpoint needs `Authorization: Bearer`.
//! The SDK would put an inline config on the command line, where `ps` shows it
//! (and the SDK logs the command at debug level), so a token-bearing config is
//! written to an owner-only temp file and passed by path instead. The URL never
//! carries a secret.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anthropic_agent_sdk::types::options::SystemPrompt;
use anthropic_agent_sdk::types::{McpHttpServerConfig, McpServerConfig, McpServers};
use anthropic_agent_sdk::ClaudeAgentOptions;

/// Name the document-bound server is registered under. Its tools reach the
/// model as `mcp__openpencil-doc__<tool>`.
pub const BOUND_SERVER_NAME: &str = "openpencil-doc";

/// The tool-name prefix the agent sees for the bound server.
pub fn bound_tool_prefix() -> String {
    format!("mcp__{BOUND_SERVER_NAME}__")
}

/// Where a chat turn reaches ITS document's canvas.
#[derive(Clone, PartialEq, Eq)]
pub struct McpEndpoint {
    url: String,
    bearer: Option<String>,
}

impl McpEndpoint {
    /// `url` is the document route (`mcp_url_for_document`), never carrying a
    /// secret; `bearer` is the token when the server requires one.
    pub fn new(url: impl Into<String>, bearer: Option<String>) -> Self {
        Self {
            url: url.into(),
            bearer,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn has_bearer(&self) -> bool {
        self.bearer.is_some()
    }

    /// `Authorization: Bearer <token>` when a token is held, else no headers.
    fn headers(&self) -> HashMap<String, String> {
        self.bearer
            .iter()
            .map(|token| ("Authorization".to_string(), format!("Bearer {token}")))
            .collect()
    }

    fn server_config(&self) -> McpServerConfig {
        McpServerConfig::Http(McpHttpServerConfig {
            server_type: "http".into(),
            url: self.url.clone(),
            // Always an object: the SDK serialises `None` as `null`.
            headers: Some(self.headers()),
        })
    }

    fn config_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mcpServers": {
                BOUND_SERVER_NAME: {
                    "type": "http",
                    "url": self.url,
                    "headers": self.headers(),
                }
            }
        })
    }
}

impl std::fmt::Debug for McpEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpEndpoint")
            .field("url", &self.url)
            .field("bearer", &self.bearer.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Removes the owner-only config directory when the turn ends.
pub(super) struct McpConfigGuard {
    dir: PathBuf,
}

impl Drop for McpConfigGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The line appended to the turn's system prompt.
fn bound_prompt_note() -> String {
    format!(
        "OPENPENCIL DOCUMENT: this chat belongs to one open document. Use the `{prefix}*` tools \
         for the canvas: they are bound to that document. Do not use the unbound \
         `mcp__openpencil__*` tools for this turn.",
        prefix = bound_tool_prefix()
    )
}

/// Register the document-bound server on `options`. `None` changes nothing.
///
/// Options that already carry MCP servers are left alone (nothing in-tree sets
/// them; a caller-supplied configuration wins). A failure to stage the token
/// file also leaves `options` untouched, which is today's unbound behaviour; the
/// error is logged without the token.
pub(super) fn apply_document_mcp(
    options: &mut ClaudeAgentOptions,
    endpoint: Option<&McpEndpoint>,
) -> Option<McpConfigGuard> {
    let endpoint = endpoint?;
    let free = match &options.mcp_servers {
        McpServers::None => true,
        McpServers::Dict(servers) => servers.is_empty(),
        McpServers::Path(_) => false,
    };
    if !free {
        return None;
    }
    let guard = if endpoint.has_bearer() {
        match write_config_file(endpoint) {
            Ok((path, guard)) => {
                options.mcp_servers = McpServers::Path(path);
                Some(guard)
            }
            Err(error) => {
                eprintln!("openpencil claude chat: cannot stage the MCP config ({error})");
                return None;
            }
        }
    } else {
        let servers = HashMap::from([(BOUND_SERVER_NAME.to_string(), endpoint.server_config())]);
        options.mcp_servers = McpServers::Dict(servers);
        None
    };
    let note = bound_prompt_note();
    options.system_prompt = match options.system_prompt.take() {
        None => Some(SystemPrompt::String(note)),
        Some(SystemPrompt::String(existing)) if existing.trim().is_empty() => {
            Some(SystemPrompt::String(note))
        }
        Some(SystemPrompt::String(existing)) => {
            Some(SystemPrompt::String(format!("{existing}\n\n{note}")))
        }
        // A preset or file prompt cannot be appended to here.
        other => other,
    };
    guard
}

static CONFIG_SEQ: AtomicU64 = AtomicU64::new(0);

fn write_config_file(endpoint: &McpEndpoint) -> std::io::Result<(PathBuf, McpConfigGuard)> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = CONFIG_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-claude-mcp-{}-{stamp}-{seq}",
        std::process::id()
    ));
    create_private_dir(&dir)?;
    let guard = McpConfigGuard { dir: dir.clone() };
    let path = dir.join("mcp.json");
    let mut file = create_private_file(&path)?;
    file.write_all(endpoint.config_json().to_string().as_bytes())?;
    Ok((path, guard))
}

#[cfg(unix)]
fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

#[cfg(unix)]
fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
#[path = "chat_claude_mcp_tests.rs"]
mod tests;
