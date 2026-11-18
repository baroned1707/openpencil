//! Pixabay against a loopback fake: the request shape, the hit mapping, the
//! 24 h cache and the 100-per-60 s window. Nothing here reaches the network.

use super::*;
use crate::net::providers::{ImageProvider, ProviderCtx};
use crate::net::test_support::{query_param, test_client, FakeHttp, FakeResponse};

use std::time::{Duration, Instant};

const KEY: &str = "pixabay-test-key-123";

fn pixabay_hit_json(id: u64, base_url: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "pageURL": format!("https://pixabay.com/photos/flowers-{id}/"),
        "previewURL": format!("{base_url}/preview/{id}.jpg"),
        "webformatURL": format!("{base_url}/web/{id}.png"),
        "largeImageURL": format!("{base_url}/large/{id}.png"),
        "imageURL": format!("{base_url}/approval-only/{id}.png"),
        "fullHDURL": format!("{base_url}/approval-only-hd/{id}.png"),
        "vectorURL": format!("{base_url}/approval-only-vector/{id}.svg"),
        "imageWidth": 4000,
        "imageHeight": 3000,
        "user": "Jane Photographer",
        "tags": "flowers, nature, bloom",
    })
}

fn fake_pixabay() -> FakeHttp {
    FakeHttp::start(|target, _| {
        if target.starts_with("/api/") {
            let base = "http://pixabay.invalid";
            FakeResponse::json(serde_json::json!({
                "total": 2,
                "totalHits": 2,
                "hits": [pixabay_hit_json(11, base), pixabay_hit_json(12, base)],
            }))
        } else {
            FakeResponse::status(404)
        }
    })
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    crate::net::block_on_image_runtime(future)
}

#[allow(clippy::too_many_arguments)]
fn fetch(
    runtime: &StockRuntime,
    base_url: &str,
    query: &str,
    kind: ImageKind,
    aspect: Option<ImageAspectRatio>,
    now: Instant,
) -> Result<serde_json::Value, SearchError> {
    run(fetch_list_json(
        &test_client(),
        runtime,
        base_url,
        &ApiKey::new(KEY),
        query,
        kind,
        aspect,
        20,
        now,
    ))
}

fn fresh_runtime() -> StockRuntime {
    StockRuntime::new(REQUESTS_PER_WINDOW, WINDOW, CACHE_TTL, 256)
}

#[test]
fn pixabay_hit_maps_to_hit_with_credit() {
    let json = serde_json::json!({"hits": [pixabay_hit_json(11, "https://cdn.example")]});
    let hits = parse_pixabay_results(&json);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.id, "11");
    assert_eq!(hit.provider, "pixabay");
    assert_eq!(hit.creator.as_deref(), Some("Jane Photographer"));
    assert_eq!(hit.license.as_deref(), Some("Pixabay Content License"));
    assert_eq!(
        hit.license_url.as_deref(),
        Some("https://pixabay.com/service/license-summary/")
    );
    assert_eq!(
        hit.source_page_url.as_deref(),
        Some("https://pixabay.com/photos/flowers-11/")
    );
    assert!(hit.relevance_metadata.contains("flowers"));
    let credit = hit.provenance().expect("a pixabay hit carries a credit");
    assert_eq!(credit.provider, "pixabay");
    assert_eq!(credit.creator.as_deref(), Some("Jane Photographer"));
    // The named licence passes the policy gate on its own terms.
    assert!(crate::net::providers::hit_license_is_usable(hit));
    assert!(hit.download_ping.is_none() && !hit.thumb_is_svg);
}

#[test]
fn pixabay_uses_webformat_not_approval_only_urls() {
    let base = "https://cdn.example";
    let json = serde_json::json!({"hits": [pixabay_hit_json(7, base)]});
    let hit = &parse_pixabay_results(&json)[0];
    assert_eq!(hit.thumb_url, format!("{base}/web/7.png"));
    assert_eq!(
        hit.original.as_ref().map(|original| original.url.as_str()),
        Some(format!("{base}/large/7.png").as_str())
    );
    for url in [
        Some(hit.thumb_url.as_str()),
        hit.original.as_ref().map(|original| original.url.as_str()),
        hit.source_page_url.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(!url.contains("approval-only"), "{url}");
    }
    // A hit without a webformat URL is not usable at all.
    let json = serde_json::json!({"hits": [{"id": 1, "imageURL": "https://x/approval.png"}]});
    assert!(parse_pixabay_results(&json).is_empty());
}

#[test]
fn request_shape_follows_the_documented_limits() {
    let long_query: String = "é".repeat(150);
    let url = search_url(
        "https://pixabay.com",
        KEY,
        &long_query,
        ImageKind::Photo,
        Some(ImageAspectRatio::Wide),
        1,
    )
    .unwrap();
    let target = format!("{}?{}", url.path(), url.query().unwrap());
    assert_eq!(query_param(&target, "key").as_deref(), Some(KEY));
    // 100 characters (not bytes), cut on a character boundary.
    assert_eq!(query_param(&target, "q").unwrap().chars().count(), 100);
    // The test button's page size of 1 is raised to Pixabay's minimum of 3.
    assert_eq!(query_param(&target, "per_page").as_deref(), Some("3"));
    assert_eq!(query_param(&target, "image_type").as_deref(), Some("photo"));
    assert_eq!(
        query_param(&target, "orientation").as_deref(),
        Some("horizontal")
    );
    assert_eq!(query_param(&target, "safesearch").as_deref(), Some("true"));

    let url = search_url(
        "https://pixabay.com",
        KEY,
        "cat",
        ImageKind::Vector,
        Some(ImageAspectRatio::Square),
        500,
    )
    .unwrap();
    let target = format!("{}?{}", url.path(), url.query().unwrap());
    assert_eq!(query_param(&target, "per_page").as_deref(), Some("200"));
    assert_eq!(
        query_param(&target, "image_type").as_deref(),
        Some("vector")
    );
    assert!(query_param(&target, "orientation").is_none());
}

#[test]
fn pixabay_query_cache_respects_24h_and_injectable_clock() {
    let server = fake_pixabay();
    let runtime = fresh_runtime();
    let start = Instant::now();
    let hour = Duration::from_secs(60 * 60);

    fetch(
        &runtime,
        &server.base_url,
        "flowers",
        ImageKind::Photo,
        None,
        start,
    )
    .unwrap();
    assert_eq!(server.requests().len(), 1);
    // Within 24 h the same (query, kind, orientation) is answered locally.
    fetch(
        &runtime,
        &server.base_url,
        "flowers",
        ImageKind::Photo,
        None,
        start + hour * 23,
    )
    .unwrap();
    assert_eq!(server.requests().len(), 1, "cached within 24 h");
    // A different kind or orientation is a different key.
    fetch(
        &runtime,
        &server.base_url,
        "flowers",
        ImageKind::Vector,
        None,
        start + hour,
    )
    .unwrap();
    fetch(
        &runtime,
        &server.base_url,
        "flowers",
        ImageKind::Photo,
        Some(ImageAspectRatio::Tall),
        start + hour,
    )
    .unwrap();
    assert_eq!(server.requests().len(), 3);
    // After 24 h the entry has expired and the host is asked again.
    fetch(
        &runtime,
        &server.base_url,
        "flowers",
        ImageKind::Photo,
        None,
        start + hour * 25,
    )
    .unwrap();
    assert_eq!(server.requests().len(), 4, "expired after 24 h");
}

#[test]
fn pixabay_window_blocks_the_101st_request_in_60s() {
    let server = fake_pixabay();
    let runtime = fresh_runtime();
    let start = Instant::now();
    for index in 0..100 {
        fetch(
            &runtime,
            &server.base_url,
            &format!("query {index}"),
            ImageKind::Photo,
            None,
            start,
        )
        .unwrap_or_else(|err| panic!("request {index} must be allowed: {err:?}"));
    }
    assert_eq!(server.requests().len(), 100);
    // The 101st distinct query inside the minute is skipped without a request.
    let blocked = fetch(
        &runtime,
        &server.base_url,
        "query 100",
        ImageKind::Photo,
        None,
        start + Duration::from_secs(59),
    );
    assert_eq!(blocked.unwrap_err(), SearchError::Skipped);
    assert_eq!(server.requests().len(), 100);
    // A cached answer still works while the window is full.
    fetch(
        &runtime,
        &server.base_url,
        "query 5",
        ImageKind::Photo,
        None,
        start + Duration::from_secs(59),
    )
    .unwrap();
    // The window slides after 60 s.
    fetch(
        &runtime,
        &server.base_url,
        "query 100",
        ImageKind::Photo,
        None,
        start + Duration::from_secs(61),
    )
    .unwrap();
    assert_eq!(server.requests().len(), 101);
}

#[test]
fn a_429_retry_after_blocks_later_requests_and_repeats_skip_the_provider() {
    let server = FakeHttp::start(|_, _| FakeResponse::status(429).with_header("Retry-After", "30"));
    let runtime = fresh_runtime();
    let start = Instant::now();
    let first = fetch(
        &runtime,
        &server.base_url,
        "a",
        ImageKind::Photo,
        None,
        start,
    );
    assert_eq!(first.unwrap_err(), SearchError::RequestFailed);
    // Inside Retry-After nothing is sent.
    let second = fetch(
        &runtime,
        &server.base_url,
        "b",
        ImageKind::Photo,
        None,
        start + Duration::from_secs(10),
    );
    assert_eq!(second.unwrap_err(), SearchError::Skipped);
    assert_eq!(server.requests().len(), 1);
    // Throttled three times in a row: skipped for the process.
    for (index, query) in ["c", "d"].into_iter().enumerate() {
        let now = start + Duration::from_secs(31 + 31 * index as u64);
        let _ = fetch(
            &runtime,
            &server.base_url,
            query,
            ImageKind::Photo,
            None,
            now,
        );
    }
    let after = fetch(
        &runtime,
        &server.base_url,
        "e",
        ImageKind::Photo,
        None,
        start + Duration::from_secs(200),
    );
    assert_eq!(after.unwrap_err(), SearchError::Skipped);
    assert_eq!(server.requests().len(), 3);
}

#[test]
fn provider_search_end_to_end_filters_by_licence_and_hides_the_key() {
    let server = fake_pixabay();
    let provider = ImageProvider::pixabay_at(&server.base_url, ApiKey::new(KEY));
    // `{:?}` of the provider and of a request context never shows the key.
    assert!(!format!("{provider:?}").contains(KEY));
    assert!(provider.needs_key());
    assert_eq!(provider.id(), "pixabay");
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let hits = run(provider.search(&ctx, "end to end flowers", None)).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|hit| hit.provider == "pixabay"));
    let sent = &server.requests()[0];
    assert_eq!(query_param(sent, "key").as_deref(), Some(KEY));
}

#[test]
fn a_blank_key_never_sends_a_request() {
    let server = fake_pixabay();
    let provider = ImageProvider::pixabay_at(&server.base_url, ApiKey::new("   "));
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    assert!(run(provider.search(&ctx, "flowers", None)).is_err());
    assert!(server.requests().is_empty());
}
