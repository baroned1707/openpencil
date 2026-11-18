//! Unsplash transport, parser and the download ping
//! (`GET /search/photos`, `links.download_location`).
//!
//! Unsplash is opt-in (see `providers_for`): its API terms restrict how a key
//! may be used, so it only runs with the user's own access key AND an
//! explicit switch. Rules followed here (Unsplash API guidelines): the
//! `Authorization: Client-ID <key>` header carries the key (never the URL),
//! a demo key is limited to 50 requests per hour, credit links carry
//! `utm_source=openpencil&utm_medium=referral`, and when a photo is actually
//! USED the photo's `download_location` is requested once
//! ([`send_download_ping`], fired by the apply seam, never by a listing).
//! Embedding the downloaded bytes instead of hot-linking `urls.*` is a
//! deliberate product choice that Unsplash's guidelines may not allow; see
//! the report.

use std::time::{Duration, Instant};

use op_editor_core::ApiKey;
use reqwest::header::AUTHORIZATION;

use super::catalog::{read_json_capped, text_field, RawHit};
use super::pacing::{is_skipped, note_success, note_throttled, retry_after_seconds};
use super::provider::SearchError;
use super::stock_pacing::{runtime_for, StockRuntime};
use crate::license_policy::{UNSPLASH_LICENSE, UNSPLASH_LICENSE_URL};
use crate::ImageAspectRatio;

pub(crate) const REQUESTS_PER_WINDOW: usize = 50;
pub(crate) const WINDOW: Duration = Duration::from_secs(60 * 60);
const MAX_PER_PAGE: usize = 30;
const UTM: &str = "utm_source=openpencil&utm_medium=referral";
const ACCEPT_VERSION: &str = "v1";

pub(crate) fn runtime(base_url: &str) -> std::sync::Arc<StockRuntime> {
    // Unsplash answers are not cached: nothing in its guidelines asks for it
    // and the 50/hour window already bounds the request count.
    runtime_for(base_url, REQUESTS_PER_WINDOW, WINDOW, Duration::ZERO, 1)
}

fn authorization(key: &str) -> String {
    format!("Client-ID {key}")
}

pub(crate) fn search_url(
    base_url: &str,
    query: &str,
    aspect: Option<ImageAspectRatio>,
    per_page: usize,
) -> Option<reqwest::Url> {
    let per_page = per_page.clamp(1, MAX_PER_PAGE).to_string();
    let mut url = reqwest::Url::parse_with_params(
        &format!("{}/search/photos", base_url.trim_end_matches('/')),
        &[
            ("query", query),
            ("page", "1"),
            ("per_page", per_page.as_str()),
            ("content_filter", "high"),
        ],
    )
    .ok()?;
    if let Some(aspect) = aspect {
        url.query_pairs_mut().append_pair(
            "orientation",
            match aspect {
                ImageAspectRatio::Wide => "landscape",
                ImageAspectRatio::Tall => "portrait",
                ImageAspectRatio::Square => "squarish",
            },
        );
    }
    Some(url)
}

/// One Unsplash query as raw JSON. A full 50-per-hour window, a `Retry-After`
/// still running or repeated throttling is `Err(Skipped)`.
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
        .expect("unsplash window poisoned")
        .try_acquire_at(now)
    {
        eprintln!("[ENRICH] unsplash: request window full or Retry-After running, skipping");
        return Err(SearchError::Skipped);
    }
    let resp = client
        .get(url)
        .header(AUTHORIZATION, authorization(key))
        .header("Accept-Version", ACCEPT_VERSION)
        .send()
        .await
        .map_err(|err| {
            eprintln!(
                "[ENRICH] unsplash: request failed: {}",
                super::catalog::error_chain(&err.without_url())
            );
            SearchError::RequestFailed
        })?;
    let status = resp.status();
    // Unsplash answers 403 (not only 429) once the hourly quota is spent.
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status == reqwest::StatusCode::FORBIDDEN
    {
        let wait = Duration::from_secs(retry_after_seconds(resp.headers()));
        runtime
            .window
            .lock()
            .expect("unsplash window poisoned")
            .block_for_at(now, wait);
        eprintln!("[ENRICH] unsplash: HTTP {status} (rate limited or key refused)");
        return Err(if note_throttled(base_url) {
            SearchError::Skipped
        } else {
            SearchError::RequestFailed
        });
    }
    if !status.is_success() {
        eprintln!("[ENRICH] unsplash: HTTP {status}");
        return Err(SearchError::RequestFailed);
    }
    note_success(base_url);
    read_json_capped(resp)
        .await
        .filter(|json| json.get("results").is_some_and(serde_json::Value::is_array))
        .ok_or(SearchError::RequestFailed)
}

/// Settings > Images Test: a 401 (bad key) or any failure is Invalid. Does
/// not touch the hourly window: a Test press must not eat user quota twice.
pub(crate) async fn probe(client: &reqwest::Client, base_url: &str, key: &ApiKey) -> bool {
    let (Some(key), Some(url)) = (key.trimmed(), search_url(base_url, "test", None, 1)) else {
        return false;
    };
    match client
        .get(url)
        .header(AUTHORIZATION, authorization(key))
        .header("Accept-Version", ACCEPT_VERSION)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => read_json_capped(resp)
            .await
            .is_some_and(|json| json.get("results").is_some_and(serde_json::Value::is_array)),
        _ => false,
    }
}

/// Tell Unsplash a photo was used (their download-tracking rule). The body is
/// ignored. `Ok(())` for any 2xx.
pub async fn send_download_ping(
    client: &reqwest::Client,
    download_location: &str,
    key: &ApiKey,
) -> Result<(), String> {
    let key = key.trimmed().ok_or("no Unsplash key")?;
    let resp = client
        .get(download_location)
        .header(AUTHORIZATION, authorization(key))
        .header("Accept-Version", ACCEPT_VERSION)
        .send()
        .await
        .map_err(|err| super::catalog::error_chain(&err.without_url()))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

/// `url` with the referral parameters Unsplash asks credit links to carry.
fn with_utm(url: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{UTM}")
}

/// Same scheme, host and port as `base_url`: the download ping carries the
/// access key, so it may only go back to the host that answered the search.
fn same_origin(url: &str, base_url: &str) -> bool {
    match (reqwest::Url::parse(url), reqwest::Url::parse(base_url)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

pub fn parse_unsplash_results(json: &serde_json::Value, base_url: &str) -> Vec<RawHit> {
    let Some(results) = json.get("results").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|photo| unsplash_hit(photo, base_url))
        .collect()
}

fn unsplash_hit(photo: &serde_json::Value, base_url: &str) -> Option<RawHit> {
    let urls = photo.get("urls")?;
    let thumb = text_field(urls, "regular")?;
    let id = text_field(photo, "id")?;
    let alt = text_field(photo, "alt_description").unwrap_or_default();
    let description = text_field(photo, "description").unwrap_or_default();
    let tags: Vec<String> = photo
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| text_field(tag, "title"))
                .collect()
        })
        .unwrap_or_default();
    let creator = photo.get("user").and_then(|user| text_field(user, "name"));
    let page = photo
        .get("links")
        .and_then(|links| text_field(links, "html"))
        .map(|page| with_utm(&page));
    let ping = photo
        .get("links")
        .and_then(|links| text_field(links, "download_location"))
        .filter(|location| same_origin(location, base_url));
    let title = if alt.is_empty() {
        description.clone()
    } else {
        alt.clone()
    };
    Some(RawHit {
        id,
        thumb_url: thumb,
        attribution: creator
            .as_deref()
            .map(|name| format!("Photo by {name} on Unsplash"))
            .unwrap_or_else(|| "Unsplash".to_string()),
        relevance_metadata: [alt, description, tags.join(" ")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
        title,
        provider: "unsplash",
        creator,
        license: Some(UNSPLASH_LICENSE.to_string()),
        license_url: Some(UNSPLASH_LICENSE_URL.to_string()),
        source_page_url: page,
        download_ping: ping,
        ..RawHit::default()
    })
}

#[cfg(test)]
#[path = "unsplash_tests.rs"]
mod tests;
