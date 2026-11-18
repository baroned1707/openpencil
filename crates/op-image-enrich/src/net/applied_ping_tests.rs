//! Unsplash's download ping against a loopback fake: sent once per APPLIED
//! image, never for a listing, and a failing ping cannot fail the apply.

use super::*;
use crate::net::providers::{run_search_with_providers, ImageProvider};
use crate::net::test_support::{test_client, FakeHttp, FakeResponse};
use crate::ImageKind;

use std::time::{Duration, Instant};

const KEY: &str = "unsplash-ping-key";

fn photo(id: &str, base: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "alt_description": "red bicycle",
        "urls": {"regular": format!("{base}/thumb/{id}.png")},
        "user": {"name": "Uma"},
        "links": {
            "html": format!("https://unsplash.com/photos/{id}"),
            "download_location": format!("{base}/photos/{id}/download"),
        },
    })
}

fn fake_unsplash(ping_status: u16) -> FakeHttp {
    let base = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let handler_base = base.clone();
    let server = FakeHttp::start(move |target, _| {
        let base = handler_base.lock().unwrap().clone();
        if target.starts_with("/search/photos") {
            FakeResponse::json(
                serde_json::json!({"results": [photo("u1", &base), photo("u2", &base)]}),
            )
        } else if target.starts_with("/thumb/") {
            FakeResponse::png()
        } else if target.ends_with("/download") {
            FakeResponse::status(ping_status)
        } else {
            FakeResponse::status(404)
        }
    });
    *base.lock().unwrap() = server.base_url.clone();
    server
}

fn pings(server: &FakeHttp) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter(|target| target.ends_with("/download"))
        .collect()
}

/// Wait up to two seconds for `want` pings (they run on a worker thread).
fn wait_for_pings(server: &FakeHttp, want: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while pings(server).len() < want && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    pings(server)
}

fn slot_state() -> op_editor_core::EditorState {
    let document = serde_json::from_value(serde_json::json!({
        "version": "1.0.0",
        "children": [{"type": "image", "id": "slot", "name": "Bike", "width": 64,
                      "height": 64, "src": ""}]
    }))
    .unwrap();
    op_editor_core::EditorState::from_document(document)
}

fn found(url: &str, ping: Option<String>) -> FoundImage {
    FoundImage {
        url: url.to_string(),
        credit: Some(crate::ImageCredit {
            provider: "unsplash".into(),
            creator: Some("Uma".into()),
            license: Some("Unsplash License".into()),
            license_url: Some("https://unsplash.com/license".into()),
            source_page_url: None,
        }),
        download_ping: ping,
    }
}

fn png_data_url(tag: u8) -> String {
    use base64::Engine as _;
    let mut bytes = FakeResponse::png().body;
    bytes.push(tag);
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[test]
fn unsplash_ping_sent_once_per_applied_image_only() {
    let server = fake_unsplash(200);
    let provider = ImageProvider::unsplash_at(&server.base_url, op_editor_core::ApiKey::new(KEY));
    let client = test_client();

    // Listing two photos sends no ping, however many results come back.
    let outcome = crate::net::block_on_image_runtime(run_search_with_providers(
        &client,
        &[provider],
        ImageKind::Photo,
        "red bicycle",
        None,
        |url: String| {
            let client = client.clone();
            async move { crate::net::fetch::fetch_image_data_url(&client, &url).await }
        },
    ));
    assert_eq!(outcome.results.len(), 2, "both photos are listed");
    assert!(outcome
        .results
        .iter()
        .all(|hit| hit.download_ping.is_some()));
    std::thread::sleep(Duration::from_millis(150));
    assert!(pings(&server).is_empty(), "a listing must not ping");

    // Applying one of them pings exactly once, with the key in the header.
    let chosen = &outcome.results[0];
    let mut state = slot_state();
    let image = found(&png_data_url(1), chosen.download_ping.clone());
    let key = op_editor_core::ApiKey::new(KEY);
    assert!(apply_found_image_with_ping(
        &mut state,
        &op_editor_core::NodeId::new("slot"),
        &image,
        &key
    ));
    let sent = wait_for_pings(&server, 1);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("/photos/u1/download"));
    let head = server
        .request_heads()
        .into_iter()
        .find(|head| head.contains("/download"))
        .unwrap()
        .to_ascii_lowercase();
    assert!(head.contains(&format!(
        "authorization: client-id {}",
        KEY.to_ascii_lowercase()
    )));

    // Re-applying the identical result changes nothing and pings nothing.
    assert!(!apply_found_image_with_ping(
        &mut state,
        &op_editor_core::NodeId::new("slot"),
        &image,
        &key
    ));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(pings(&server).len(), 1, "no second ping for the same apply");

    // A different applied image pings once more; the unused second listing
    // (u2 was never applied) never did.
    let next = found(
        &png_data_url(2),
        Some(format!("{}/photos/u9/download", server.base_url)),
    );
    assert!(apply_found_image_with_ping(
        &mut state,
        &op_editor_core::NodeId::new("slot"),
        &next,
        &key
    ));
    let sent = wait_for_pings(&server, 2);
    assert_eq!(sent.len(), 2);
    assert!(!sent.iter().any(|target| target.contains("/u2/")));
}

#[test]
fn nothing_is_pinged_without_a_location_or_a_key() {
    let key = op_editor_core::ApiKey::new(KEY);
    assert!(notify_applied(None, &key).is_none());
    assert!(notify_applied(Some("  "), &key).is_none());
    assert!(notify_applied(
        Some("http://127.0.0.1:1/x"),
        &op_editor_core::ApiKey::new(" ")
    )
    .is_none());
}

#[test]
fn failed_unsplash_ping_does_not_fail_apply() {
    let key = op_editor_core::ApiKey::new(KEY);
    // The host answers HTTP 500 to the ping.
    let broken = fake_unsplash(500);
    let mut state = slot_state();
    let image = found(
        &png_data_url(3),
        Some(format!("{}/photos/u1/download", broken.base_url)),
    );
    assert!(apply_found_image_with_ping(
        &mut state,
        &op_editor_core::NodeId::new("slot"),
        &image,
        &key
    ));
    assert_eq!(
        wait_for_pings(&broken, 1).len(),
        1,
        "the ping was attempted"
    );
    assert!(
        op_editor_core::node_image_credit(&state.active_children()[0]).is_some(),
        "the image and its credit landed regardless"
    );

    // The host is gone entirely (connection refused): still no panic or error.
    let mut state = slot_state();
    let image = found(
        &png_data_url(4),
        Some("http://127.0.0.1:1/photos/u1/download".to_string()),
    );
    assert!(apply_found_image_with_ping(
        &mut state,
        &op_editor_core::NodeId::new("slot"),
        &image,
        &key
    ));
    let handle = notify_applied(Some("http://127.0.0.1:1/photos/u1/download"), &key).unwrap();
    assert!(!handle.join().expect("the worker never panics"));
}
