//! Writes `~/.openpencil/.op-mcp-port` so the Rust `op` CLI can discover
//! this running editor's live MCP server.
//!
//! This is a SEPARATE file from the TS app's `~/.openpencil/.port` because
//! the two hosts speak different wire protocols: the Rust editor serves
//! JSON-RPC at `/mcp`, while the TS app serves REST at `/api/mcp/*`. To make
//! the two cross-discoverable (transport-unification, step 4 of that plan),
//! the record now carries an explicit `transport` field (`"json-rpc"` here;
//! the TS writer publishes `"rest"`), so a CLI that finds the *other*
//! stack's file can read `transport` and speak the right protocol instead
//! of mis-routing. The content is JSON
//! `{port, pid, writerPid, token, timestamp, transport}`; the CLI reads
//! `port`/`pid`/`token`/`transport` and always confirms identity with a
//! `ping` (matching `token`) before trusting it or terminating the pid. We
//! only ever remove a file we ourselves wrote (`writerPid` guard) so a
//! second instance can't delete the live instance's discovery file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Name of the bearer-token file, beside `settings.json`.
const BEARER_TOKEN_FILE: &str = "mcp-token";
/// The token is 32 random bytes, spelled as lowercase hex.
const BEARER_TOKEN_BYTES: usize = 32;

fn port_file_path() -> Option<PathBuf> {
    // A test must never publish into (or remove from) the real `~/.openpencil`.
    crate::test_config_root::guard_user_config();
    op_config_store::ConfigStore::user()
        .ok()?
        .path(op_config_store::well_known::LIVE_MCP_PORT)
        .ok()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Publish the live MCP server on `port` (with identity `token`) under
/// `~/.openpencil/.op-mcp-port`. Best-effort: failures are non-fatal
/// (discovery falls back to the fixed default port on the CLI side).
pub(crate) fn write(port: u16, token: &str) {
    let Some(path) = port_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let pid = std::process::id();
    let body = port_record_json(port, pid, token, now_millis());
    // Owner-only: it carries the token `openpencil/shutdown` accepts.
    let _ = write_owner_only_atomically(&path, body.as_bytes());
}

/// Build the discovery-file JSON record. Pure (no I/O / no env) so it's
/// unit-testable. `transport: "json-rpc"` advertises this host's wire
/// protocol for cross-stack discovery (the TS writer publishes `"rest"`).
pub(crate) fn port_record_json(port: u16, pid: u32, token: &str, timestamp: u64) -> String {
    serde_json::json!({
        "port": port,
        "pid": pid,
        "writerPid": pid,
        "token": token,
        "timestamp": timestamp,
        "transport": "json-rpc",
    })
    .to_string()
}

/// Remove `~/.openpencil/.op-mcp-port`, but only if we wrote it (matching
/// `writerPid`). A missing or foreign file is left untouched.
pub(crate) fn remove() {
    let Some(path) = port_file_path() else {
        return;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    let writer_pid = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.get("writerPid").and_then(serde_json::Value::as_u64));
    if writer_pid == Some(u64::from(std::process::id())) {
        let _ = std::fs::remove_file(&path);
    }
}

/// The bearer token the live MCP server requires when `mcp_require_token` is
/// on, read from (or first written to) `<settings dir>/mcp-token`.
///
/// One token for the life of the install: external CLIs keep the copy their
/// config file holds, so it must survive restarts. `None` when this machine has
/// no settings directory.
pub(crate) fn bearer_token() -> Option<std::io::Result<String>> {
    crate::test_config_root::guard_user_config();
    let dir = op_host_services::settings_io::settings_dir()?;
    Some(load_or_create_bearer_token(&dir))
}

/// [`bearer_token`] in an explicit directory (tests inject a temp one).
///
/// A file holding anything but a well-formed token is replaced, and a file
/// found with group/world permissions is tightened to `0600`: the token is a
/// credential, so it is never left readable by other accounts.
pub(crate) fn load_or_create_bearer_token(dir: &Path) -> std::io::Result<String> {
    let path = dir.join(BEARER_TOKEN_FILE);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if is_well_formed_token(existing) {
            restrict_to_owner(&path)?;
            return Ok(existing.to_string());
        }
    }
    std::fs::create_dir_all(dir)?;
    let token = new_bearer_token()?;
    write_owner_only_atomically(&path, token.as_bytes())?;
    Ok(token)
}

/// 32 random bytes as 64 lowercase hex characters.
pub(crate) fn new_bearer_token() -> std::io::Result<String> {
    let mut bytes = [0u8; BEARER_TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_well_formed_token(text: &str) -> bool {
    text.len() == BEARER_TOKEN_BYTES * 2
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Write `bytes` to a `0600` sibling temp file, then rename it over `path`, so
/// the token never exists on disk with wider permissions and a crash never
/// leaves half a token.
fn write_owner_only_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options
        .open(&temp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode();
    if mode & 0o077 != 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_record_advertises_json_rpc_transport() {
        let record = port_record_json(3100, 4242, "tok-1", 1_700_000_000_000);
        let value: serde_json::Value = serde_json::from_str(&record).expect("valid json");
        assert_eq!(value["port"], 3100);
        assert_eq!(value["pid"], 4242);
        assert_eq!(value["writerPid"], 4242);
        assert_eq!(value["token"], "tok-1");
        assert_eq!(value["timestamp"], 1_700_000_000_000u64);
        // The cross-stack discovery marker: a CLI reading this file knows to
        // speak JSON-RPC (vs the TS app's `.port` which carries "rest").
        assert_eq!(value["transport"], "json-rpc");
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("openpencil-mcp-token-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_token_is_32_random_bytes_created_once_and_then_reused() {
        let dir = scratch("create");
        let first = load_or_create_bearer_token(&dir).expect("create");
        assert_eq!(first.len(), 64, "{first}");
        assert!(is_well_formed_token(&first));
        assert_eq!(load_or_create_bearer_token(&dir).expect("reuse"), first);
        assert_ne!(new_bearer_token().expect("fresh"), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_token_file_is_owner_only_and_a_lax_one_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("mode");
        let token = load_or_create_bearer_token(&dir).expect("create");
        let path = dir.join(BEARER_TOKEN_FILE);
        let mode =
            |path: &Path| std::fs::metadata(path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("loosen");
        assert_eq!(load_or_create_bearer_token(&dir).expect("reuse"), token);
        assert_eq!(mode(&path), 0o600, "found readable by others, tightened");
        let leftovers: Vec<_> = std::fs::read_dir(&dir).expect("list").collect();
        assert_eq!(leftovers.len(), 1, "no temp file left: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_token_file_is_replaced_not_trusted() {
        let dir = scratch("malformed");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join(BEARER_TOKEN_FILE), "short").expect("seed");
        let token = load_or_create_bearer_token(&dir).expect("replace");
        assert!(is_well_formed_token(&token));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
