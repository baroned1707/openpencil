//! Browser image-search route for the web daemon.
//!
//! `POST /api/ai/image/search` mirrors the desktop image panel's Search
//! popover backend (`op-host-desktop/src/image_panel_host.rs`: Openverse →
//! two-keyword retry → Wikimedia, thumbnails embedded as `data:` URLs) so
//! the wasm shell can drain its `search_epoch` through the daemon instead
//! of leaving the popover loading forever. Openverse credentials come from
//! the request body (browser-held) or fall back to the daemon's persisted
//! agent settings. Openverse / Wikimedia are product-constant public hosts
//! — the same operator-trust tier as the desktop path — so they dial with
//! a plain client; nothing in this route dials a browser-supplied URL.
//!
//! Unlike the desktop, fetched thumbnails are NOT re-encoded/down-scaled
//! here: `image_downscale` needs skia and this crate must stay GL-free for
//! `op-host-web-server`. The 4 MiB per-image cap still bounds what can be
//! embedded.

mod brand_fence;
mod catalog;
mod download;
mod iconify;
mod ladder;
pub mod license;
mod openverse;
mod original_download;
pub(crate) mod pacing;
mod pexels;
mod pixabay;
mod provider;
mod relevance;
pub mod ssrf_guard;
mod stock_pacing;
mod unsplash;
mod wikimedia;

pub(crate) use catalog::original_source;
pub(crate) use catalog::wikimedia_hit;
pub(crate) use catalog::wikimedia_info_is_image;
pub use catalog::{parse_openverse_results, parse_wikimedia_results, OriginalImageSource, RawHit};
pub use download::{
    fetch_image_and_judge_thumbnail, fetch_image_and_judge_thumbnail_for_view, fetch_image_bytes,
    fetch_image_data_url, fetch_judge_thumbnail, normalize_image_mime_header, read_capped,
    sniff_image_mime,
};
pub(crate) use iconify::fetch_svg_data_url;
pub(crate) use ladder::{fenced_list, run_ladder, FencedList, LadderRequest};
pub(crate) use license::hit_license_is_usable;
pub use license::{
    iconify_spdx_policy, license_policy, license_policy_with, provider_license_policy,
    LicensePolicy,
};
pub use openverse::fetch_openverse_token;
pub(crate) use original_download::fetch_original_image_data_url;
pub(crate) use provider::WIKIMEDIA_BASE_URL;
pub use provider::{
    default_providers, providers_for, test_first_provider, ImageProvider, ProviderCtx,
    ProviderKind, SearchError, StockKeys,
};
pub use relevance::simplify_search_query;
pub use unsplash::send_download_ping;
pub(crate) use wikimedia::fetch_pages_json as fetch_wikimedia_pages_json;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::ImageKind;
use catalog::{materialize_first_thumb, materialize_thumbs};
pub(crate) use relevance::{
    fence_rejection, retain_relevant_hits_for_intent, retrieval_plan, two_keyword_retry,
};

#[cfg(test)]
pub(crate) use ladder::fenced_openverse_list_with;
#[cfg(test)]
use ladder::{fetch_relevant_openverse_list_with, fetch_relevant_wikimedia_list_with};
#[cfg(test)]
use relevance::retain_relevant_hits;
#[cfg(test)]
pub(crate) use relevance::rewrite_queries;
#[cfg(test)]
use relevance::{
    core_query_words, metadata_is_scene_heavy, query_requests_scene, SCENE_HEAVY_RESULT_WORDS,
};

/// Cap on concurrently running image jobs (search + generate combined).
/// Each job blocks one connection thread for up to minutes of provider
/// network; without a ceiling a page could exhaust the daemon's threads.
const MAX_IN_FLIGHT_IMAGE_JOBS: usize = 4;

static IN_FLIGHT_IMAGE_JOBS: AtomicUsize = AtomicUsize::new(0);

/// RAII slot for one running image job. `acquire` fails once
/// [`MAX_IN_FLIGHT_IMAGE_JOBS`] jobs are running (route answers 429).
pub struct ImageJobSlot(());

impl ImageJobSlot {
    pub fn acquire() -> Option<Self> {
        IN_FLIGHT_IMAGE_JOBS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_IN_FLIGHT_IMAGE_JOBS).then_some(n + 1)
            })
            .ok()
            .map(|_| Self(()))
    }
}

impl Drop for ImageJobSlot {
    fn drop(&mut self) {
        IN_FLIGHT_IMAGE_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// TS popover requests `count: 5` (desktop parity).
const SEARCH_RESULT_COUNT: usize = 5;
/// Fetch a wider catalogue window before relevance ranking. The public route
/// still materializes at most [`SEARCH_RESULT_COUNT`] thumbnails.
const SEARCH_CANDIDATE_COUNT: usize = 20;
pub const MAX_EMBEDDED_IMAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct WebOpenverseCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl WebOpenverseCredentials {
    /// `None` unless both parts are non-empty after trimming.
    pub fn from_parts(client_id: &str, client_secret: &str) -> Option<Self> {
        let client_id = client_id.trim();
        let client_secret = client_secret.trim();
        if client_id.is_empty() || client_secret.is_empty() {
            None
        } else {
            Some(Self {
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
            })
        }
    }
}

/// One search hit ready for the JSON reply / the desktop popover.
pub struct WebImageSearchHit {
    pub id: String,
    pub thumb_data_url: String,
    pub attribution: String,
    /// Creator / licence record persisted on the node when the hit is chosen;
    /// `None` for a hand-built hit that names no provider.
    pub credit: Option<crate::ImageCredit>,
    /// Unsplash `download_location`, pinged once when the hit is APPLIED
    /// (never when merely listed); not persisted anywhere.
    pub download_ping: Option<String>,
}

pub struct WebImageSearchOutcome {
    pub results: Vec<WebImageSearchHit>,
    /// `"openverse"` / `"wikimedia"`, `None` when nothing landed.
    pub source: Option<&'static str>,
}

/// Why a `POST /api/ai/image/search` body was refused. Both variants answer
/// HTTP 400; the enum exists so the route reports WHICH client mistake was
/// made instead of matching on prose, and `Display` reproduces the exact
/// sentence the JSON reply already carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchRequestError {
    /// The body is not JSON, or is JSON but not an object.
    InvalidBody,
    /// The body is a valid object but carries no non-blank `query`.
    MissingQuery,
}

impl std::fmt::Display for SearchRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchRequestError::InvalidBody => f.write_str("invalid request body"),
            SearchRequestError::MissingQuery => f.write_str("missing query"),
        }
    }
}

impl std::error::Error for SearchRequestError {}

/// Parse the request body and snapshot the daemon-side credential fallback.
/// Returns `(query, credentials)` or the reason for the 400 reply.
pub fn parse_search_request(
    body: &str,
    state: &op_editor_core::EditorState,
) -> Result<(String, Option<WebOpenverseCredentials>), SearchRequestError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| SearchRequestError::InvalidBody)?;
    let obj = value.as_object().ok_or(SearchRequestError::InvalidBody)?;
    let query = obj
        .get("query")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or(SearchRequestError::MissingQuery)?;
    // Browser-held credential wins; the daemon's persisted settings are the
    // fallback (both are optional — anonymous Openverse works, rate-limited).
    let request_credentials = obj
        .get("openverse")
        .and_then(serde_json::Value::as_object)
        .and_then(|cred| {
            WebOpenverseCredentials::from_parts(
                cred.get("client_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                cred.get("client_secret")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
            )
        });
    let credentials = request_credentials.or_else(|| {
        let settings = &state.editor_ui.agent_settings;
        WebOpenverseCredentials::from_parts(
            &settings.openverse_client_id,
            &settings.openverse_client_secret,
        )
    });
    Ok((query.to_string(), credentials))
}

/// JSON reply body for a finished search.
pub fn search_outcome_to_json(outcome: &WebImageSearchOutcome) -> String {
    let results: Vec<serde_json::Value> = outcome
        .results
        .iter()
        .map(|hit| {
            let mut value = serde_json::json!({
                "id": hit.id,
                "thumb_data_url": hit.thumb_data_url,
                "attribution": hit.attribution,
            });
            // Only present when a provider reported one, so a reply without
            // credits stays byte-identical to the previous shape.
            if let Some(credit) = &hit.credit {
                value["credit"] = serde_json::to_value(credit).unwrap_or_default();
            }
            value
        })
        .collect();
    serde_json::json!({
        "ok": true,
        "results": results,
        "source": outcome.source,
    })
    .to_string()
}

/// Run the full search ladder on the calling thread (the connection's own
/// thread — the caller must NOT hold the state lock).
pub fn run_search_blocking(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> WebImageSearchOutcome {
    // A private runtime here would panic the moment this sync helper is
    // reached from a tokio worker; `block_on_anywhere` runs the ladder on the
    // shared (enable_all) runtime instead — same IO/timer drivers, no
    // runtime-in-runtime hazard.
    crate::net::block_on_image_runtime(run_search(query, credentials))
}

/// Run the provider ladder for a single usable thumbnail, bounding the whole
/// async ladder (catalog requests, retries, and thumbnail download together)
/// by `remaining`. This is the MCP enrichment path: unlike the Web UI search,
/// it needs only one image and must return before its outer transport timeout.
pub fn run_first_search_blocking_with_timeout(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    remaining: Duration,
) -> WebImageSearchOutcome {
    run_first_search_blocking_with_providers(
        &default_providers(),
        ImageKind::Photo,
        query,
        credentials,
        remaining,
    )
}

/// [`run_first_search_blocking_with_timeout`] over the caller's provider
/// list and slot kind (the MCP `enrich_images` path with the user's keys).
pub fn run_first_search_blocking_with_providers(
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    remaining: Duration,
) -> WebImageSearchOutcome {
    run_with_timeout(
        remaining,
        run_first_search(providers, kind, query, credentials),
    )
    .unwrap_or_else(empty_search_outcome)
}

/// Judge-aware single-result variant used by headless enrichment callers.
/// The ordinary function above intentionally keeps its existing materializer
/// for the no-judge path and for the image-search popover.
pub fn run_first_search_blocking_with_judge(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    remaining: Duration,
    judge: &dyn crate::net::ImageRelevanceJudge,
    intent: &str,
) -> WebImageSearchOutcome {
    run_with_timeout(
        remaining,
        run_first_search_with_judge(query, credentials, judge, intent),
    )
    .unwrap_or_else(empty_search_outcome)
}

fn run_with_timeout<F>(remaining: Duration, future: F) -> Option<F::Output>
where
    F: std::future::Future,
{
    if remaining.is_zero() {
        return None;
    }
    crate::net::block_on_image_runtime(
        async move { tokio::time::timeout(remaining, future).await.ok() },
    )
}

fn empty_search_outcome() -> WebImageSearchOutcome {
    WebImageSearchOutcome {
        results: Vec::new(),
        source: None,
    }
}

/// Plain client for the catalogue ladders: rustls, 8 s budget, the shared
/// contact user-agent.
fn ladder_client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(8))
        .user_agent(crate::HTTP_USER_AGENT)
        .build()
        .ok()
}

async fn run_search(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> WebImageSearchOutcome {
    let Some(client) = ladder_client() else {
        return empty_search_outcome();
    };
    run_search_with_fetcher(&client, query, credentials, |url: String| {
        let client = client.clone();
        async move { fetch_image_data_url(&client, &url).await }
    })
    .await
}

/// Single-result variant of [`run_search`]. Provider list lookup keeps the
/// same ladder order, but thumbnail materialization stops after the first
/// successful download instead of fetching the whole five-result Web UI page.
async fn run_first_search(
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> WebImageSearchOutcome {
    let Some(client) = ladder_client() else {
        return empty_search_outcome();
    };
    run_first_search_over_kind(
        &client,
        providers,
        kind,
        query,
        credentials,
        |url: String| {
            let client = client.clone();
            async move { fetch_image_data_url(&client, &url).await }
        },
    )
    .await
}

async fn run_first_search_with_judge(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    judge: &dyn crate::net::ImageRelevanceJudge,
    intent: &str,
) -> WebImageSearchOutcome {
    let Some(client) = ladder_client() else {
        return empty_search_outcome();
    };
    run_first_search_with_judge_over(
        &client,
        &default_providers(),
        query,
        credentials,
        judge,
        intent,
    )
    .await
}

/// [`run_first_search_with_judge`] over a caller-supplied client + provider
/// list. Only Openverse is judged here (its ten best thumbnails go to the
/// judge); every other provider keeps the first-usable-thumbnail pick this
/// path has always used for Wikimedia. The list fence takes the EMPTY intent
/// (as it always did on this path); `intent` only reaches the judge.
pub(crate) async fn run_first_search_with_judge_over(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    judge: &dyn crate::net::ImageRelevanceJudge,
    intent: &str,
) -> WebImageSearchOutcome {
    let query = simplify_search_query(query);
    let ctx = ProviderCtx::new(client, credentials);
    let request = LadderRequest {
        query: &query,
        intent: "",
        aspect: None,
    };
    let fetch_data_url = |url: String| {
        let client = client.clone();
        async move { fetch_image_data_url(&client, &url).await }
    };
    let query_ref = query.as_str();
    let fetch_data_url = &fetch_data_url;
    let found = run_ladder(providers, &ctx, &request, |provider, list| {
        let source = provider.id();
        let judged = matches!(provider, ImageProvider::Openverse { .. });
        async move {
            let result = if judged {
                materialize_judged_first(client, list.hits, query_ref, intent, judge).await
            } else {
                materialize_first_thumb(client, list.hits, fetch_data_url).await
            };
            result.map(|hit| (hit, source))
        }
    })
    .await;
    outcome_from_first(found)
}

fn outcome_from_first(found: Option<(WebImageSearchHit, &'static str)>) -> WebImageSearchOutcome {
    match found {
        Some((hit, source)) => WebImageSearchOutcome {
            results: vec![hit],
            source: Some(source),
        },
        None => empty_search_outcome(),
    }
}

/// The full search ladder over a caller-supplied client + thumbnail
/// materializer. Shared by this daemon route (plain embed) and the desktop
/// popover (its own user-agent + skia down-scale pass on each thumbnail).
///
/// `fetch_data_url` downloads one thumbnail URL into a `data:` URL; hits
/// whose thumbnails fail to download are dropped.
pub async fn run_search_with_fetcher<F, Fut>(
    client: &reqwest::Client,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    run_search_with_providers(
        client,
        &default_providers(),
        ImageKind::Photo,
        query,
        credentials,
        fetch_data_url,
    )
    .await
}

/// [`run_search_with_fetcher`] over the caller's provider list (built with
/// [`providers_for`] from the user's keys) and slot kind.
pub async fn run_search_with_providers<F, Fut>(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    run_search_over_kind(client, providers, kind, query, credentials, fetch_data_url).await
}

#[cfg(test)]
pub(crate) async fn run_search_over<F, Fut>(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    run_search_over_kind(
        client,
        providers,
        ImageKind::Photo,
        query,
        credentials,
        fetch_data_url,
    )
    .await
}

pub(crate) async fn run_search_over_kind<F, Fut>(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    // Simplify verbose prompts into keywords (TS simplifySearchQuery).
    let query = simplify_search_query(query);
    let ctx = ProviderCtx::new(client, credentials).with_kind(kind);
    let request = LadderRequest {
        query: &query,
        intent: "",
        aspect: None,
    };
    let fetch_data_url = &fetch_data_url;
    let found = run_ladder(providers, &ctx, &request, |provider, list| {
        let source = provider.id();
        async move {
            let results = materialize_thumbs(client, list.hits, fetch_data_url).await;
            (!results.is_empty()).then_some((results, source))
        }
    })
    .await;
    match found {
        Some((results, source)) => WebImageSearchOutcome {
            results,
            source: Some(source),
        },
        None => empty_search_outcome(),
    }
}

#[cfg(test)]
pub(crate) async fn run_first_search_over<F, Fut>(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    run_first_search_over_kind(
        client,
        providers,
        ImageKind::Photo,
        query,
        credentials,
        fetch_data_url,
    )
    .await
}

/// MCP / headless single-result ladder over a caller-supplied provider list.
pub(crate) async fn run_first_search_over_kind<F, Fut>(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let query = simplify_search_query(query);
    let ctx = ProviderCtx::new(client, credentials).with_kind(kind);
    let request = LadderRequest {
        query: &query,
        intent: "",
        aspect: None,
    };
    let fetch_data_url = &fetch_data_url;
    let found = run_ladder(providers, &ctx, &request, |provider, list| {
        let source = provider.id();
        async move {
            materialize_first_thumb(client, list.hits, fetch_data_url)
                .await
                .map(|hit| (hit, source))
        }
    })
    .await;
    outcome_from_first(found)
}

struct PreparedJudgeHit {
    hit: RawHit,
    thumb_jpeg: Vec<u8>,
    data_url: String,
}

async fn materialize_judged_first(
    client: &reqwest::Client,
    hits: Vec<RawHit>,
    query: &str,
    intent: &str,
    judge: &dyn crate::net::ImageRelevanceJudge,
) -> Option<WebImageSearchHit> {
    const ROUND_SIZE: usize = 5;
    const MAX_JUDGE_CANDIDATES: usize = ROUND_SIZE * 2;
    let mut prepared = Vec::new();
    for hit in hits.into_iter().take(MAX_JUDGE_CANDIDATES) {
        let Some((data_url, thumb_jpeg)) =
            fetch_image_and_judge_thumbnail(client, &hit.thumb_url).await
        else {
            continue;
        };
        prepared.push(PreparedJudgeHit {
            hit,
            thumb_jpeg,
            data_url,
        });
    }
    let batches: Vec<Vec<Vec<u8>>> = prepared
        .chunks(ROUND_SIZE)
        .map(|batch| {
            batch
                .iter()
                .map(|candidate| candidate.thumb_jpeg.clone())
                .collect()
        })
        .collect();
    let selection = crate::net::select_with_judge(judge, query, intent, &batches);
    eprintln!("[ENRICH] {}", crate::net::format_judge_log(&selection));
    let index = selection.ordered_indices().first().copied()?;
    let candidate = prepared.get(index)?;
    Some(WebImageSearchHit {
        id: candidate.hit.id.clone(),
        thumb_data_url: candidate.data_url.clone(),
        attribution: candidate.hit.attribution.clone(),
        credit: candidate.hit.provenance(),
        download_ping: candidate.hit.download_ping.clone(),
    })
}

#[cfg(test)]
#[path = "web_image_search_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "web_image_search_relevance_tests.rs"]
mod relevance_tests;

#[cfg(test)]
#[path = "web_image_search_retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
#[path = "web_image_search_ladder_seam_tests.rs"]
mod ladder_seam_tests;

#[cfg(test)]
#[path = "provider_ladder_tests.rs"]
mod provider_ladder_tests;

#[cfg(test)]
#[path = "stock_ladder_tests.rs"]
mod stock_ladder_tests;
