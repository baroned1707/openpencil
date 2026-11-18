//! The single seam where an applied stock image is reported back to its
//! provider. Today that is Unsplash's "download" ping: a photo must be
//! reported once when it is actually USED, never when it is merely listed.
//!
//! Every host that lands a [`FoundImage`] on a node calls [`notify_applied`]
//! once per APPLIED image (after the write reports a change). The ping runs
//! on its own thread, the apply never waits for it, and a failure is only
//! logged.

use std::thread::JoinHandle;

use op_editor_core::{ApiKey, EditorState, NodeId};

use super::providers::send_download_ping;
use crate::FoundImage;

/// Report `download_ping` (when the hit has one) to its provider with `key`.
/// `None` when there is nothing to report: no ping URL, or no key. The
/// returned handle exists so tests can wait for the thread; production
/// callers drop it.
pub fn notify_applied(download_ping: Option<&str>, key: &ApiKey) -> Option<JoinHandle<bool>> {
    let location = download_ping?.trim();
    if location.is_empty() || key.is_blank() {
        return None;
    }
    let (location, key) = (location.to_string(), key.clone());
    Some(std::thread::spawn(move || {
        let Some(client) = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent(crate::HTTP_USER_AGENT)
            .build()
            .ok()
        else {
            return false;
        };
        match super::block_on_image_runtime(send_download_ping(&client, &location, &key)) {
            Ok(()) => true,
            Err(reason) => {
                eprintln!("[ENRICH] unsplash: download ping failed: {reason}");
                false
            }
        }
    }))
}

/// [`crate::apply_result_with_credit`] for a [`FoundImage`], plus the ping
/// once the write reports a change. For hosts without their own apply
/// wrapper (the desktop adds a caption repair and calls [`notify_applied`]).
pub fn apply_found_image_with_ping(
    state: &mut EditorState,
    node_id: &NodeId,
    found: &FoundImage,
    key: &ApiKey,
) -> bool {
    let changed =
        crate::apply_result_with_credit(state, node_id, &found.url, found.credit.as_ref());
    if changed {
        notify_applied(found.download_ping.as_deref(), key);
    }
    changed
}

#[cfg(test)]
#[path = "applied_ping_tests.rs"]
mod tests;
