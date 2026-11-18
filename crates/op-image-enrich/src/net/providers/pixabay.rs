//! Pixabay transport and parser (`GET /api/`).
//!
//! Rules this module follows (Pixabay API docs): the query is at most 100
//! characters, `per_page` is 3..=200, results are cached for 24 hours, the
//! limit is 100 requests per 60 seconds, and image URLs are not hot-linked
//! permanently (the editor downloads and embeds the bytes). Only the URLs
//! every key may use are read: `webformatURL` (640 px) for the thumbnail and
//! `largeImageURL` (1280 px) for the upgrade. `imageURL`, `fullHDURL` and
//! `vectorURL` need Pixabay's approval and are never used.
//!
//! The key travels in the query string (Pixabay's only option), so no log
//! line here prints a URL or a `reqwest` error that still carries it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use op_editor_core::ApiKey;

use super::catalog::{read_json_capped, text_field, OriginalImageSource, RawHit};
use super::pacing::{is_skipped, note_success, note_throttled, retry_after_seconds};
use super::provider::SearchError;
use super::stock_pacing::{runtime_for, StockRuntime};
use crate::license_policy::{PIXABAY_LICENSE, PIXABAY_LICENSE_URL};
use crate::{ImageAspectRatio, ImageKind};

pub(crate) const REQUESTS_PER_WINDOW: usize = 100;
pub(crate) const WINDOW: Duration = Duration::from_secs(60);
pub(crate) const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const CACHE_CAPACITY: usize = 256;
const MAX_QUERY_CHARS: usize = 100;
const MIN_PER_PAGE: usize = 3;
const MAX_PER_PAGE: usize = 200;

pub(crate) fn runtime(base_url: &str) -> Arc<StockRuntime> {
    runtime_for(
        base_url,
        REQUESTS_PER_WINDOW,
        WINDOW,
        CACHE_TTL,
        CACHE_CAPACITY,
    )
}

/// `q` capped at 100 characters on a character boundary.
fn capped_query(query: &str) -> String {
    query.chars().take(MAX_QUERY_CHARS).collect()
}

pub(crate) fn search_url(
    base_url: &str,
    key: &str,
    query: &str,
    kind: ImageKind,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
) -> Option<reqwest::Url> {
    let per_page = per_page.clamp(MIN_PER_PAGE, MAX_PER_PAGE).to_string();
    let image_type = match kind {
        ImageKind::Photo => "photo",
        ImageKind::Vector => "vector",
    };
    let mut url = reqwest::Url::parse_with_params(
        &format!("{}/api/", base_url.trim_end_matches('/')),
        &[
            ("key", key),
            ("q", capped_query(query).as_str()),
            ("image_type", image_type),
            ("per_page", per_page.as_str()),
            ("safesearch", "true"),
        ],
    )
    .ok()?;
    if let Some(orientation) = orientation_param(aspect) {
        url.query_pairs_mut()
            .append_pair("orientation", orientation);
    }
    Some(url)
}

/// Pixabay only distinguishes horizontal from vertical; a square slot takes
/// either.
fn orientation_param(aspect: Option<ImageAspectRatio>) -> Option<&'static str> {
    match aspect? {
        ImageAspectRatio::Wide => Some("horizontal"),
        ImageAspectRatio::Tall => Some("vertical"),
        ImageAspectRatio::Square => None,
    }
}

fn cache_key(
    base_url: &str,
    query: &str,
    kind: ImageKind,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
) -> String {
    format!(
        "{base_url}|{}|{}|{per_page}|{}",
        kind.as_str(),
        orientation_param(aspect).unwrap_or("any"),
        capped_query(query)
    )
}

/// One Pixabay query as raw JSON.
///
/// A cached answer (24 h, keyed by query, kind, orientation and page size)
/// costs no request. Otherwise a request slot is reserved first: a full
/// 100-per-60-second window, a server `Retry-After` still running, or a
/// provider throttled three times in a row is `Err(Skipped)`, so the ladder
/// moves to the next provider instead of sleeping.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_list_json(
    client: &reqwest::Client,
    runtime: &StockRuntime,
    base_url: &str,
    key: &ApiKey,
    query: &str,
    kind: ImageKind,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
    now: Instant,
) -> Result<serde_json::Value, SearchError> {
    let Some(key) = key.trimmed() else {
        return Err(SearchError::RequestFailed);
    };
    let cache_key = cache_key(base_url, query, kind, aspect, per_page);
    if let Some(cached) = runtime
        .cache
        .lock()
        .expect("pixabay cache poisoned")
        .get_at(&cache_key, now)
    {
        return Ok(cached);
    }
    if is_skipped(base_url) {
        return Err(SearchError::Skipped);
    }
    let url = search_url(base_url, key, query, kind, aspect, per_page)
        .ok_or(SearchError::RequestFailed)?;
    if !runtime
        .window
        .lock()
        .expect("pixabay window poisoned")
        .try_acquire_at(now)
    {
        eprintln!("[ENRICH] pixabay: request window full or Retry-After running, skipping");
        return Err(SearchError::Skipped);
    }
    let resp = match client.get(url).send().await {
        Ok(resp) => resp,
        Err(err) => {
            // `without_url`: the URL carries the key.
            eprintln!(
                "[ENRICH] pixabay: request failed: {}",
                super::catalog::error_chain(&err.without_url())
            );
            return Err(SearchError::RequestFailed);
        }
    };
    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait = Duration::from_secs(retry_after_seconds(resp.headers()));
        runtime
            .window
            .lock()
            .expect("pixabay window poisoned")
            .block_for_at(now, wait);
        eprintln!(
            "[ENRICH] pixabay: HTTP 429 (retry-after={}s)",
            wait.as_secs()
        );
        return Err(if note_throttled(base_url) {
            SearchError::Skipped
        } else {
            SearchError::RequestFailed
        });
    }
    if !status.is_success() {
        eprintln!("[ENRICH] pixabay: HTTP {status}");
        return Err(SearchError::RequestFailed);
    }
    note_success(base_url);
    let json = read_json_capped(resp)
        .await
        .filter(|json| json.get("hits").is_some_and(serde_json::Value::is_array))
        .ok_or(SearchError::RequestFailed)?;
    runtime
        .cache
        .lock()
        .expect("pixabay cache poisoned")
        .insert_at(cache_key, json.clone(), now);
    Ok(json)
}

/// Settings > Images Test: one tiny uncached query. Valid only for a 2xx
/// answer that carries `hits` (a wrong key answers HTTP 400).
pub(crate) async fn probe(client: &reqwest::Client, base_url: &str, key: &ApiKey) -> bool {
    let Some(url) = key
        .trimmed()
        .and_then(|key| search_url(base_url, key, "test", ImageKind::Photo, None, MIN_PER_PAGE))
    else {
        return false;
    };
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_success() => read_json_capped(resp)
            .await
            .is_some_and(|json| json.get("hits").is_some_and(serde_json::Value::is_array)),
        _ => false,
    }
}

pub fn parse_pixabay_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(hits) = json.get("hits").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    hits.iter().filter_map(pixabay_hit).collect()
}

fn pixabay_hit(hit: &serde_json::Value) -> Option<RawHit> {
    let thumb = text_field(hit, "webformatURL")?;
    let id = hit.get("id").map(|id| match id.as_str() {
        Some(text) => text.to_string(),
        None => id.to_string(),
    })?;
    let tags = text_field(hit, "tags").unwrap_or_default();
    let creator = text_field(hit, "user");
    Some(RawHit {
        id,
        original: text_field(hit, "largeImageURL")
            .filter(|large| *large != thumb)
            .map(|url| OriginalImageSource {
                url,
                width: None,
                height: None,
            }),
        attribution: creator
            .as_deref()
            .map(|user| format!("Image by {user} from Pixabay"))
            .unwrap_or_else(|| "Pixabay".to_string()),
        title: tags.clone(),
        relevance_metadata: tags,
        thumb_url: thumb,
        provider: "pixabay",
        creator,
        license: Some(PIXABAY_LICENSE.to_string()),
        license_url: Some(PIXABAY_LICENSE_URL.to_string()),
        source_page_url: text_field(hit, "pageURL"),
        ..RawHit::default()
    })
}

#[cfg(test)]
#[path = "pixabay_tests.rs"]
mod tests;
