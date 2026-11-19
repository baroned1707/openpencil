//! Pexels against a loopback fake: request shape and header auth, the result
//! mapping with its credit and named licence, the aspect and size filters, and
//! the pacing rules. Nothing here reaches the real network.

use super::*;
use crate::net::providers::{ImageProvider, ProviderCtx};
use crate::net::test_support::{query_param, test_client, FakeHttp, FakeResponse};

const KEY: &str = "pexels-api-key-789";

fn photo_json(id: u64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "width": 6000,
        "height": 4000,
        "url": format!("https://www.pexels.com/photo/red-bicycle-{id}/"),
        "photographer": "Pia Pixel",
        "photographer_url": "https://www.pexels.com/@pia",
        "photographer_id": 77,
        "avg_color": "#A1B2C3",
        "src": {
            "original": format!("https://images.pexels.com/photos/{id}/original.jpeg"),
            "large2x": format!("https://images.pexels.com/photos/{id}/p.jpeg?dpr=2&h=650&w=940"),
            "large": format!("https://images.pexels.com/photos/{id}/p.jpeg?h=650&w=940"),
            "medium": format!("https://images.pexels.com/photos/{id}/p.jpeg?h=350"),
        },
        "alt": "A red bicycle leaning on a wall",
    })
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    crate::net::block_on_image_runtime(future)
}

fn fresh_runtime() -> StockRuntime {
    StockRuntime::new(REQUESTS_PER_WINDOW, WINDOW, Duration::ZERO, 1)
}

#[test]
fn pexels_hit_maps_credit_and_the_named_licence() {
    let json = serde_json::json!({"photos": [photo_json(2014422)], "total_results": 1});
    let hits = parse_pexels_results(&json);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.provider, "pexels");
    assert_eq!(hit.id, "2014422");
    assert_eq!(hit.creator.as_deref(), Some("Pia Pixel"));
    assert_eq!(hit.license.as_deref(), Some("Pexels License"));
    assert_eq!(
        hit.license_url.as_deref(),
        Some("https://www.pexels.com/license/")
    );
    assert_eq!(
        hit.source_page_url.as_deref(),
        Some("https://www.pexels.com/photo/red-bicycle-2014422/")
    );
    assert_eq!(hit.attribution, "Photo by Pia Pixel on Pexels");
    assert_eq!(hit.title, "A red bicycle leaning on a wall");
    assert!(hit.relevance_metadata.contains("red bicycle"));
    // The embedded picture is `large`, the upgrade `large2x`; `original`
    // (up to 24 MP) is never read, and Pexels has no download ping.
    assert!(hit.thumb_url.contains("h=650&w=940") && !hit.thumb_url.contains("dpr=2"));
    assert!(hit.original.as_ref().unwrap().url.contains("dpr=2"));
    assert!(hit.download_ping.is_none());
    let credit = hit.provenance().unwrap();
    assert_eq!(credit.provider, "pexels");
    assert_eq!(credit.creator.as_deref(), Some("Pia Pixel"));
    assert_eq!(
        credit.source_page_url.as_deref(),
        Some("https://www.pexels.com/photo/red-bicycle-2014422/")
    );
    assert_eq!(
        credit.license_url.as_deref(),
        Some("https://www.pexels.com/license/")
    );
    assert!(crate::net::providers::hit_license_is_usable(hit));
}

#[test]
fn a_photo_without_a_large_picture_or_id_is_skipped() {
    let mut no_src = photo_json(1);
    no_src["src"] = serde_json::json!({});
    let mut no_id = photo_json(2);
    no_id.as_object_mut().unwrap().remove("id");
    let json = serde_json::json!({"photos": [no_src, no_id, photo_json(3)]});
    let hits = parse_pexels_results(&json);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "3");
    assert!(parse_pexels_results(&serde_json::json!({"nothing": []})).is_empty());
}

#[test]
fn request_carries_the_raw_key_in_a_header_never_the_url() {
    let server =
        FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"photos": [], "page": 1})));
    let provider = ImageProvider::pexels_at(&server.base_url, ApiKey::new(KEY));
    assert!(!format!("{provider:?}").contains(KEY));
    assert!(provider.needs_key());
    assert_eq!(provider.id(), "pexels");
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let hits = run(provider.search(&ctx, "red bicycle", Some(ImageAspectRatio::Square))).unwrap();
    assert!(hits.is_empty());
    let target = &server.requests()[0];
    assert!(target.starts_with("/v1/search?"));
    assert!(!target.contains(KEY));
    assert_eq!(query_param(target, "query").as_deref(), Some("red bicycle"));
    assert_eq!(
        query_param(target, "orientation").as_deref(),
        Some("square")
    );
    assert_eq!(query_param(target, "size").as_deref(), Some("small"));
    assert!(query_param(target, "per_page").is_some());
    let head = server.request_heads()[0].to_ascii_lowercase();
    // Pexels takes the bare key: no `Client-ID` / `Bearer` prefix.
    assert!(head.contains(&format!("authorization: {}\r\n", KEY.to_ascii_lowercase())));
}

#[test]
fn aspect_maps_to_pexels_orientations_and_per_page_is_capped() {
    for (aspect, expected) in [
        (Some(ImageAspectRatio::Wide), Some("landscape")),
        (Some(ImageAspectRatio::Tall), Some("portrait")),
        (Some(ImageAspectRatio::Square), Some("square")),
        (None, None),
    ] {
        let url = search_url("https://api.pexels.com", "q", aspect, 10).unwrap();
        assert_eq!(
            query_param(
                &format!("{}?{}", url.path(), url.query().unwrap()),
                "orientation"
            )
            .as_deref(),
            expected
        );
    }
    let big = search_url("https://api.pexels.com", "q", None, 500).unwrap();
    let target = format!("{}?{}", big.path(), big.query().unwrap());
    assert_eq!(query_param(&target, "per_page").as_deref(), Some("80"));
}

#[test]
fn a_rejected_key_is_an_invalid_probe() {
    let server = FakeHttp::start(|_, _| FakeResponse::status(401));
    let client = test_client();
    assert!(!run(probe(&client, &server.base_url, &ApiKey::new("bad"))));
    let ok = FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"photos": []})));
    assert!(run(probe(&client, &ok.base_url, &ApiKey::new(KEY))));
    assert!(!run(probe(&client, &ok.base_url, &ApiKey::new(""))));
    assert!(ok.requests().len() == 1, "a blank key never dials");
}

#[test]
fn a_blank_key_never_sends_a_request() {
    let server = FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"photos": []})));
    let provider = ImageProvider::pexels_at(&server.base_url, ApiKey::new("   "));
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    assert!(run(provider.search(&ctx, "flowers", None)).is_err());
    assert!(server.requests().is_empty());
}

#[test]
fn the_hourly_window_holds_200_requests_then_skips_without_dialling() {
    let server = FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"photos": []})));
    let client = test_client();
    let runtime = fresh_runtime();
    let key = ApiKey::new(KEY);
    let start = Instant::now();
    for _ in 0..REQUESTS_PER_WINDOW {
        run(fetch_list_json(
            &client,
            &runtime,
            &server.base_url,
            &key,
            "q",
            None,
            15,
            start,
        ))
        .unwrap();
    }
    let over = run(fetch_list_json(
        &client,
        &runtime,
        &server.base_url,
        &key,
        "q",
        None,
        15,
        start + Duration::from_secs(60),
    ));
    assert_eq!(over.unwrap_err(), SearchError::Skipped);
    assert_eq!(server.requests().len(), REQUESTS_PER_WINDOW);
    // The window slides: an hour later a request goes out again.
    run(fetch_list_json(
        &client,
        &runtime,
        &server.base_url,
        &key,
        "q",
        None,
        15,
        start + WINDOW,
    ))
    .unwrap();
    assert_eq!(server.requests().len(), REQUESTS_PER_WINDOW + 1);
}

#[test]
fn a_429_blocks_later_requests_and_repeats_skip_the_provider() {
    let server = FakeHttp::start(|_, _| FakeResponse::status(429).with_header("Retry-After", "30"));
    let client = test_client();
    let runtime = fresh_runtime();
    let key = ApiKey::new(KEY);
    let start = Instant::now();
    let fetch = |at: Instant| {
        run(fetch_list_json(
            &client,
            &runtime,
            &server.base_url,
            &key,
            "q",
            None,
            15,
            at,
        ))
    };
    assert_eq!(fetch(start).unwrap_err(), SearchError::RequestFailed);
    // Inside Retry-After nothing is sent.
    assert_eq!(
        fetch(start + Duration::from_secs(10)).unwrap_err(),
        SearchError::Skipped
    );
    assert_eq!(server.requests().len(), 1);
    // Throttled three times in a row: skipped for the process.
    for index in 0..2u64 {
        let _ = fetch(start + Duration::from_secs(31 + 31 * index));
    }
    assert_eq!(
        fetch(start + Duration::from_secs(200)).unwrap_err(),
        SearchError::Skipped
    );
    assert_eq!(server.requests().len(), 3);
}
