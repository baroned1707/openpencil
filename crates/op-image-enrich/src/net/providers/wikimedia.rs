//! Wikimedia Commons transport: one search request shared by the list ladder
//! and the judged ladder, so URL, user-agent, pacing and the body cap are
//! single-sourced.

use super::catalog::{error_chain, read_json_capped};
use super::pacing::{is_skipped, note_success, note_throttled, serial_lock, BackoffPolicy};
use super::provider::SearchError;

/// Commons `generator=search` request for `query`, `limit` pages, with the
/// imageinfo fields every consumer needs (url, size, mime, extmetadata).
pub(crate) fn search_url(base_url: &str, query: &str, limit: usize) -> Option<reqwest::Url> {
    let limit = limit.to_string();
    reqwest::Url::parse_with_params(
        &format!("{}/w/api.php", base_url.trim_end_matches('/')),
        &[
            ("action", "query"),
            ("generator", "search"),
            ("gsrsearch", query),
            ("gsrnamespace", "6"),
            ("gsrlimit", limit.as_str()),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime|extmetadata"),
            ("iiurlwidth", "800"),
            ("format", "json"),
            ("origin", "*"),
        ],
    )
    .ok()
}

/// One Commons search. `Ok(None)` = the request failed or answered garbage
/// (Wikimedia has always degraded to "no hits" rather than aborting the
/// ladder); `Err(Skipped)` = pacing refused to send (the host throttled us
/// repeatedly earlier in this process).
///
/// Requests to one host are SERIAL (never parallel): Commons rate-limits per
/// client and answers parallel bursts with 429. A 429 or 503 backs off
/// exponentially, or by the server's `Retry-After`, before retrying.
pub(crate) async fn fetch_pages_json(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    limit: usize,
) -> Result<Option<serde_json::Value>, SearchError> {
    fetch_pages_json_with(client, base_url, query, limit, &BackoffPolicy::DEFAULT).await
}

pub(crate) async fn fetch_pages_json_with(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    limit: usize,
    policy: &BackoffPolicy,
) -> Result<Option<serde_json::Value>, SearchError> {
    let Some(url) = search_url(base_url, query, limit) else {
        return Ok(None);
    };
    // Held across the request AND its backoff sleeps, so the next caller
    // cannot jump the queue while the host is asking us to slow down.
    let gate = serial_lock(base_url);
    let _serial = gate.lock().await;
    for attempt in 0..=policy.retries {
        if is_skipped(base_url) {
            return Err(SearchError::Skipped);
        }
        let resp = match client.get(url.clone()).send().await {
            Ok(resp) => resp,
            Err(err) => {
                eprintln!(
                    "[ENRICH] wikimedia: request failed for \"{query}\": {}",
                    error_chain(&err)
                );
                return Ok(None);
            }
        };
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        {
            eprintln!("[ENRICH] wikimedia: HTTP {status} for \"{query}\"");
            if note_throttled(base_url) {
                return Err(SearchError::Skipped);
            }
            if attempt == policy.retries {
                return Ok(None);
            }
            tokio::time::sleep(policy.delay(attempt, resp.headers())).await;
            continue;
        }
        if !status.is_success() {
            return Ok(None);
        }
        note_success(base_url);
        return Ok(read_json_capped(resp).await);
    }
    Ok(None)
}

#[cfg(test)]
#[path = "wikimedia_tests.rs"]
mod tests;
