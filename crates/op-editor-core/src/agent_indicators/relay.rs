//! Daemon → browser relay of the indicator registry (pure code motion out of
//! the parent module, which keeps the registry itself).

use super::*;

// ── Relay (daemon → browser) ────────────────────────────────────────────
//
// The registry is process-global, so a design run executing inside the
// serve-web daemon writes indicators the BROWSER's wasm process never
// sees. The daemon serves its registry as JSON (`relay_json`, behind
// `GET /api/mcp/indicators`); the web host polls it, parses with
// `parse_relay_json`, and mirrors it into the local registry with
// `apply_remote` so the shared canvas paint animates on web too.
// Reveal timestamps travel on the daemon's clock — `apply_remote`
// inserts them verbatim and the paint path's
// `rebase_external_clock_reveals` folds them onto the local clock,
// the same way desktop live-MCP reveals are handled.

/// A parsed relay snapshot. Vec-of-pairs (not maps) keeps the wire
/// order deterministic for tests; `apply_remote` doesn't care.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RemoteIndicators {
    /// Daemon-owned run epoch. A changed value starts a fresh local mirror
    /// even when polling missed the prior run's inactive transition.
    pub epoch: u64,
    pub run_active: bool,
    pub cursor_agent: Option<AgentTag>,
    pub nodes: Vec<(String, AgentTag)>,
    pub frames: Vec<(String, AgentTag)>,
    pub previews: Vec<String>,
    pub reveals: Vec<(String, u64)>,
}

/// Serialize the registry for the `/api/mcp/indicators` relay.
pub fn relay_json() -> String {
    let snap = snapshot();
    let tag_entries = |m: &HashMap<String, AgentTag>| -> Vec<serde_json::Value> {
        let mut entries: Vec<(&String, &AgentTag)> = m.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        entries
            .into_iter()
            .map(|(id, tag)| serde_json::json!({"id": id, "color": tag.color, "name": tag.name}))
            .collect()
    };
    let mut previews: Vec<&String> = snap.previews.iter().collect();
    previews.sort();
    let mut reveals: Vec<(&String, &u64)> = snap.reveals.iter().collect();
    reveals.sort_by(|a, b| a.0.cmp(b.0));
    serde_json::json!({
        "epoch": snap.epoch,
        "active": snap.run_active,
        "cursorAgent": snap.cursor_agent.as_ref().map(|tag| {
            serde_json::json!({"color": tag.color, "name": tag.name})
        }),
        "nodes": tag_entries(&snap.nodes),
        "frames": tag_entries(&snap.frames),
        "previews": previews,
        "reveals": reveals
            .into_iter()
            .map(|(id, ms)| serde_json::json!({"id": id, "ms": ms}))
            .collect::<Vec<_>>(),
    })
    .to_string()
}

/// Parse a `relay_json` body. `None` on any shape mismatch.
pub fn parse_relay_json(body: &str) -> Option<RemoteIndicators> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    // Epoch-less payloads came from the original relay protocol. Preserve
    // compatibility by treating them as one synthetic daemon epoch.
    let epoch = match v.get("epoch") {
        Some(value) => value.as_u64()?,
        None => 0,
    };
    let run_active = v.get("active")?.as_bool()?;
    let cursor_agent = match v.get("cursorAgent") {
        None | Some(serde_json::Value::Null) => None,
        Some(agent) => Some(AgentTag {
            color: agent.get("color")?.as_str()?.to_string(),
            name: agent.get("name")?.as_str()?.to_string(),
        }),
    };
    let tag_list = |key: &str| -> Vec<(String, AgentTag)> {
        v.get(key)
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| {
                        Some((
                            e.get("id")?.as_str()?.to_string(),
                            AgentTag {
                                color: e.get("color")?.as_str()?.to_string(),
                                name: e.get("name")?.as_str()?.to_string(),
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let previews = v
        .get("previews")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let reveals = v
        .get("reveals")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| Some((e.get("id")?.as_str()?.to_string(), e.get("ms")?.as_u64()?)))
                .collect()
        })
        .unwrap_or_default();
    Some(RemoteIndicators {
        epoch,
        run_active,
        cursor_agent,
        nodes: tag_list("nodes"),
        frames: tag_list("frames"),
        previews,
        reveals,
    })
}

/// Mirror a daemon registry snapshot into the local (browser) registry.
/// Returns `true` while anything is active or draining, so the caller
/// keeps its animation pump alive.
///
/// - A newly active remote epoch mirrors [`begin`] (fresh local epoch,
///   clean maps) so a stale prior relay can't bleed through even when the
///   browser misses the inactive snapshot between back-to-back runs.
/// - `nodes` / `frames` / `previews` are replaced wholesale — the
///   daemon is the authority for ownership tags.
/// - `reveals` are insert-only: an id already present locally keeps its
///   timestamp, which the paint path may have rebased onto the local
///   clock (overwriting would replay its entrance every poll).
/// - A remote run ending mirrors [`finish_if_epoch`]'s graceful drain:
///   queued reveals play out, then the overlay clears itself.
pub fn apply_remote(remote: &RemoteIndicators) -> bool {
    let mut r = lock_current();
    if remote.run_active {
        if !r.run_active || r.remote_epoch != Some(remote.epoch) {
            r.epoch = next_epoch();
            r.clear_maps();
            r.run_active = true;
            r.remote_epoch = Some(remote.epoch);
        }
        r.nodes = remote.nodes.iter().cloned().collect();
        r.frames = remote.frames.iter().cloned().collect();
        r.cursor_agent = remote.cursor_agent.clone();
        r.previews = remote.previews.iter().cloned().collect();
        for (id, ms) in &remote.reveals {
            r.reveals.entry(id.clone()).or_insert(*ms);
        }
        return true;
    }
    // An out-of-order inactive poll from the previous daemon run must not
    // tear down the newer active mirror.
    if r.run_active && r.remote_epoch == Some(remote.epoch) {
        r.run_active = false;
        r.finishing = true;
        if r.reveals.is_empty() {
            let cursor_was_visible = r.cursor_agent.is_some();
            r.clear_maps();
            r.epoch = next_epoch();
            r.needs_final_frame = cursor_was_visible;
        }
    }
    has_active_indicators(&r) || r.needs_final_frame
}

/// Latest scheduled reveal END (start + [`REVEAL_DURATION_MS`]) for `epoch`,
/// or `None` when the epoch is stale or nothing is scheduled. The
/// orchestrator's finalize gate polls this so tree-restructuring
/// (`ReplaceSubtree` id churn) waits for the cursor sweep to finish instead
/// of snapping the still-animating tail of the design in at once.
pub fn latest_reveal_end_ms(epoch: u64) -> Option<u64> {
    let r = lock_epoch(epoch)?;
    r.reveals
        .values()
        .max()
        .map(|s| s.saturating_add(REVEAL_DURATION_MS))
}
