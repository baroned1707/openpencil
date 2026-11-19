//! Pexels transport and parser (`GET /v1/search`).
//!
//! Opt-in like Unsplash (see `providers_for`): it runs only with the user's
//! own key AND an explicit switch. Rules followed here (Pexels API docs): the
//! raw key travels in an `Authorization` header (never the URL), the default
//! quota is 200 requests per hour and 20,000 per month (the hourly window is
//! enforced locally, the monthly one by the server's 429), `per_page` is at
//! most 80, `orientation` is `landscape`, `portrait` or `square`, and `size`
//! is a MINIMUM (`small` = 4 MP). The photographer is credited and the photo
//! page linked; Pexels asks for that "when possible", the licence itself
//! needs no credit.
//!
//! The image bytes are downloaded and embedded like Pixabay's: `src.large`
//! (940 px) for the thumbnail and `src.large2x` for the upgrade. `src.original`
//! can be 24 MP and is never used. There is no download-tracking call.

use std::time::{Duration, Instant};

use op_editor_core::ApiKey;
use reqwest::header::AUTHORIZATION;

use super::catalog::{read_json_capped, text_field, OriginalImageSource, RawHit};
use super::pacing::{is_skipped, note_success, note_throttled, retry_after_seconds};
use super::provider::SearchError;
use super::stock_pacing::{runtime_for, StockRuntime};
use crate::license_policy::{PEXELS_LICENSE, PEXELS_LICENSE_URL};
use crate::ImageAspectRatio;

pub(crate) const REQUESTS_PER_WINDOW: usize = 200;
pub(crate) const WINDOW: Duration = Duration::from_secs(60 * 60);
const MAX_PER_PAGE: usize = 80;
/// Smallest photo worth embedding: 4 MP, far above the 2048 px delivery cap.
const MIN_SIZE: &str = "small";

pub(crate) fn runtime(base_url: &str) -> std::sync::Arc<StockRuntime> {
    // Pexels answers are not cached: its docs ask for nothing of the kind and
    // the hourly window already bounds the request count.
    runtime_for(base_url, REQUESTS_PER_WINDOW, WINDOW, Duration::ZERO, 1)
}

pub(crate) fn search_url(
    base_url: &str,
    query: &str,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
) -> Option<reqwest::Url> {
    let per_page = per_page.clamp(1, MAX_PER_PAGE).to_string();
    let mut url = reqwest::Url::parse_with_params(
        &format!("{}/v1/search", base_url.trim_end_matches('/')),
        &[
            ("query", query),
            ("per_page", per_page.as_str()),
            ("size", MIN_SIZE),
        ],
    )
    .ok()?;
    if let Some(aspect) = aspect {
        url.query_pairs_mut().append_pair(
            "orientation",
            match aspect {
                ImageAspectRatio::Wide => "landscape",
                ImageAspectRatio::Tall => "portrait",
                ImageAspectRatio::Square => "square",
            },
        );
    }
    Some(url)
}

/// One Pexels query as raw JSON. A full hourly window, a `Retry-After` still
/// running or repeated throttling is `Err(Skipped)`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_list_json(
    client: &reqwest::Client,
    runtime: &StockRuntime,
    base_url: &str,
    key: &ApiKey,
    query: &str,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
    now: Instant,
) -> Result<serde_json::Value, SearchError> {
    let Some(key) = key.trimmed() else {
        return Err(SearchError::RequestFailed);
    };
    if is_skipped(base_url) {
        return Err(SearchError::Skipped);
    }
    let url = search_url(base_url, query, aspect, per_page).ok_or(SearchError::RequestFailed)?;
    if !runtime
        .window
        .lock()
        .expect("pexels window poisoned")
        .try_acquire_at(now)
    {
        eprintln!("[ENRICH] pexels: request window full or Retry-After running, skipping");
        return Err(SearchError::Skipped);
    }
    let resp = client
        .get(url)
        .header(AUTHORIZATION, key)
        .send()
        .await
        .map_err(|err| {
            eprintln!(
                "[ENRICH] pexels: request failed: {}",
                super::catalog::error_chain(&err.without_url())
            );
            SearchError::RequestFailed
        })?;
    let status = resp.status();
    // Pexels answers 429 once the hourly or monthly quota is spent; a bad key
    // is a plain 401.
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait = Duration::from_secs(retry_after_seconds(resp.headers()));
        runtime
            .window
            .lock()
            .expect("pexels window poisoned")
            .block_for_at(now, wait);
        eprintln!(
            "[ENRICH] pexels: HTTP 429 (retry-after={}s)",
            wait.as_secs()
        );
        return Err(if note_throttled(base_url) {
            SearchError::Skipped
        } else {
            SearchError::RequestFailed
        });
    }
    if !status.is_success() {
        eprintln!("[ENRICH] pexels: HTTP {status}");
        return Err(SearchError::RequestFailed);
    }
    note_success(base_url);
    read_json_capped(resp)
        .await
        .filter(|json| json.get("photos").is_some_and(serde_json::Value::is_array))
        .ok_or(SearchError::RequestFailed)
}

/// Settings > Images Test: a 401 (bad key) or any failure is Invalid. Does
/// not touch the hourly window: a Test press must not eat user quota twice.
pub(crate) async fn probe(client: &reqwest::Client, base_url: &str, key: &ApiKey) -> bool {
    let (Some(key), Some(url)) = (key.trimmed(), search_url(base_url, "test", None, 1)) else {
        return false;
    };
    match client.get(url).header(AUTHORIZATION, key).send().await {
        Ok(resp) if resp.status().is_success() => read_json_capped(resp)
            .await
            .is_some_and(|json| json.get("photos").is_some_and(serde_json::Value::is_array)),
        _ => false,
    }
}

pub fn parse_pexels_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(photos) = json.get("photos").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    photos.iter().filter_map(pexels_hit).collect()
}

fn pexels_hit(photo: &serde_json::Value) -> Option<RawHit> {
    let src = photo.get("src")?;
    let thumb = text_field(src, "large")?;
    let id = photo.get("id").map(|id| match id.as_str() {
        Some(text) => text.to_string(),
        None => id.to_string(),
    })?;
    let alt = text_field(photo, "alt").unwrap_or_default();
    let creator = text_field(photo, "photographer");
    Some(RawHit {
        id,
        original: text_field(src, "large2x")
            .filter(|large| *large != thumb)
            .map(|url| OriginalImageSource {
                url,
                width: None,
                height: None,
            }),
        attribution: creator
            .as_deref()
            .map(|name| format!("Photo by {name} on Pexels"))
            .unwrap_or_else(|| "Pexels".to_string()),
        title: alt.clone(),
        relevance_metadata: alt,
        thumb_url: thumb,
        provider: "pexels",
        creator,
        license: Some(PEXELS_LICENSE.to_string()),
        license_url: Some(PEXELS_LICENSE_URL.to_string()),
        source_page_url: text_field(photo, "url"),
        ..RawHit::default()
    })
}

#[cfg(test)]
#[path = "pexels_tests.rs"]
mod tests;
