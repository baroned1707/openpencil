//! Unsplash against a loopback fake: request shape and header auth, the
//! credit mapping with referral links, and the origin guard on the ping URL.

use super::*;
use crate::net::providers::{ImageProvider, ProviderCtx};
use crate::net::test_support::{query_param, test_client, FakeHttp, FakeResponse};

const KEY: &str = "unsplash-access-key-456";

fn photo_json(id: &str, base_url: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "width": 6000,
        "height": 4000,
        "alt_description": "a red bicycle leaning on a wall",
        "description": null,
        "urls": {
            "raw": format!("{base_url}/raw/{id}"),
            "full": format!("{base_url}/full/{id}"),
            "regular": format!("{base_url}/regular/{id}.png"),
            "small": format!("{base_url}/small/{id}"),
            "thumb": format!("{base_url}/thumb/{id}"),
        },
        "user": {"name": "Alex Rider", "links": {"html": "https://unsplash.com/@alex"}},
        "links": {
            "html": format!("https://unsplash.com/photos/{id}"),
            "download_location": format!("{base_url}/photos/{id}/download"),
        },
        "tags": [{"title": "bicycle"}, {"title": "street"}],
    })
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    crate::net::block_on_image_runtime(future)
}

#[test]
fn unsplash_hit_maps_credit_with_utm_links() {
    let base = "https://api.unsplash.com";
    let json = serde_json::json!({"results": [photo_json("abc123", base)]});
    let hits = parse_unsplash_results(&json, base);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.provider, "unsplash");
    assert_eq!(hit.creator.as_deref(), Some("Alex Rider"));
    assert_eq!(hit.license.as_deref(), Some("Unsplash License"));
    assert_eq!(
        hit.license_url.as_deref(),
        Some("https://unsplash.com/license")
    );
    assert_eq!(
        hit.source_page_url.as_deref(),
        Some("https://unsplash.com/photos/abc123?utm_source=openpencil&utm_medium=referral")
    );
    assert_eq!(hit.thumb_url, format!("{base}/regular/abc123.png"));
    assert_eq!(hit.attribution, "Photo by Alex Rider on Unsplash");
    assert!(hit.relevance_metadata.contains("bicycle"));
    assert_eq!(
        hit.download_ping.as_deref(),
        Some("https://api.unsplash.com/photos/abc123/download")
    );
    let credit = hit.provenance().unwrap();
    assert_eq!(credit.provider, "unsplash");
    assert_eq!(credit.creator.as_deref(), Some("Alex Rider"));
    assert!(crate::net::providers::hit_license_is_usable(hit));
    // A source page that already has a query string gets `&`, not a second `?`.
    assert_eq!(
        with_utm("https://unsplash.com/photos/x?a=1"),
        "https://unsplash.com/photos/x?a=1&utm_source=openpencil&utm_medium=referral"
    );
}

#[test]
fn a_download_location_on_another_host_is_dropped() {
    // The ping carries the access key, so it never leaves the search host.
    let mut photo = photo_json("p", "https://api.unsplash.com");
    photo["links"]["download_location"] = "https://evil.example/steal".into();
    let json = serde_json::json!({"results": [photo]});
    let hits = parse_unsplash_results(&json, "https://api.unsplash.com");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].download_ping.is_none());
}

#[test]
fn request_carries_the_key_in_a_header_never_the_url() {
    let server =
        FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"results": [], "total": 0})));
    let provider = ImageProvider::unsplash_at(&server.base_url, ApiKey::new(KEY));
    assert!(!format!("{provider:?}").contains(KEY));
    assert!(provider.needs_key());
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let hits = run(provider.search(&ctx, "red bicycle", Some(ImageAspectRatio::Tall))).unwrap();
    assert!(hits.is_empty());
    let target = &server.requests()[0];
    assert!(target.starts_with("/search/photos?"));
    assert!(!target.contains(KEY));
    assert_eq!(query_param(target, "query").as_deref(), Some("red bicycle"));
    assert_eq!(
        query_param(target, "orientation").as_deref(),
        Some("portrait")
    );
    assert_eq!(
        query_param(target, "content_filter").as_deref(),
        Some("high")
    );
    let head = server.request_heads()[0].to_ascii_lowercase();
    assert!(head.contains(&format!(
        "authorization: client-id {}",
        KEY.to_ascii_lowercase()
    )));
    assert!(head.contains("accept-version: v1"));
}

#[test]
fn a_rejected_key_is_an_invalid_probe() {
    let server = FakeHttp::start(|_, _| FakeResponse::status(401));
    let client = test_client();
    assert!(!run(probe(&client, &server.base_url, &ApiKey::new("bad"))));
    let ok = FakeHttp::start(|_, _| FakeResponse::json(serde_json::json!({"results": []})));
    assert!(run(probe(&client, &ok.base_url, &ApiKey::new(KEY))));
    assert!(!run(probe(&client, &ok.base_url, &ApiKey::new(""))));
}
