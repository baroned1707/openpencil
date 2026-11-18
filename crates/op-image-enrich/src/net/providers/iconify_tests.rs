//! Iconify against a loopback fake: parsing with per-collection licences, the
//! SVG embed and its refusals, and the vector ladder end to end.

use super::*;
use crate::net::providers::{ImageProvider, ProviderCtx};
use crate::net::test_support::{test_client, FakeHttp, FakeResponse};

const CART_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 24 24"><path fill="currentColor" d="M7 18c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z"/></svg>"#;

fn search_answer() -> serde_json::Value {
    serde_json::json!({
        "icons": ["mdi:cart", "mdi:cart-outline", "gpl-set:cart", "noname:cart", "bad id:x"],
        "collections": {
            "mdi": {
                "name": "Material Design Icons",
                "author": {"name": "Pictogrammers"},
                "license": {"title": "Apache 2.0", "spdx": "Apache-2.0",
                            "url": "https://github.com/Templarian/MaterialDesign/blob/master/LICENSE"},
            },
            "gpl-set": {
                "name": "GPL Set",
                "author": {"name": "Someone"},
                "license": {"title": "GPL 3.0", "spdx": "GPL-3.0-only", "url": "https://gnu.org/gpl"},
            },
        }
    })
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    crate::net::block_on_image_runtime(future)
}

#[test]
fn parse_maps_icons_with_collection_licences() {
    let hits = parse_iconify_results(&search_answer(), "https://api.iconify.design/");
    // The malformed id is dropped; the icon of an unknown collection is kept
    // (and refused later by the policy: no licence).
    let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "mdi:cart",
            "mdi:cart-outline",
            "gpl-set:cart",
            "noname:cart"
        ]
    );
    let cart = &hits[0];
    assert_eq!(cart.provider, "iconify");
    assert_eq!(
        cart.thumb_url,
        "https://api.iconify.design/mdi/cart.svg?height=256"
    );
    assert!(cart.thumb_is_svg);
    assert_eq!(cart.creator.as_deref(), Some("Pictogrammers"));
    assert_eq!(cart.license.as_deref(), Some("Apache 2.0"));
    assert_eq!(cart.license_id.as_deref(), Some("Apache-2.0"));
    assert_eq!(
        cart.source_page_url.as_deref(),
        Some("https://icon-sets.iconify.design/mdi/cart/")
    );
    assert!(cart.relevance_metadata.contains("cart"));
    let credit = cart.provenance().unwrap();
    assert_eq!(credit.license.as_deref(), Some("Apache 2.0"));
    // Per-collection licence policy: Apache allowed, GPL and unknown refused.
    assert!(crate::net::providers::hit_license_is_usable(&hits[0]));
    assert!(!crate::net::providers::hit_license_is_usable(&hits[2]));
    assert!(!crate::net::providers::hit_license_is_usable(&hits[3]));
}

#[test]
fn search_url_respects_the_documented_limit_floor() {
    let url = search_url("https://api.iconify.design", "cart", 1).unwrap();
    assert!(url.as_str().contains("limit=32"));
    assert!(url
        .as_str()
        .starts_with("https://api.iconify.design/search?"));
}

#[test]
fn svg_embed_accepts_plain_icons_and_refuses_active_content() {
    assert!(is_plain_icon_svg(CART_SVG));
    for hostile in [
        "<svg><script>alert(1)</script></svg>",
        "<svg><foreignObject><div/></foreignObject></svg>",
        "<svg><image href=\"data:image/png;base64,AAAA\"/></svg>",
        "<svg><use href=\"https://evil.example/x.svg#a\"/></svg>",
        "<!DOCTYPE svg [<!ENTITY a \"b\">]><svg/>",
        "not an svg at all",
    ] {
        assert!(!is_plain_icon_svg(hostile), "{hostile}");
    }
}

#[test]
fn fetch_svg_data_url_embeds_and_caps() {
    let server = FakeHttp::start(|target, _| {
        if target.starts_with("/mdi/cart.svg") {
            FakeResponse {
                status: 200,
                headers: Vec::new(),
                content_type: "image/svg+xml",
                body: CART_SVG.as_bytes().to_vec(),
            }
        } else if target.starts_with("/big/") {
            FakeResponse {
                status: 200,
                headers: Vec::new(),
                content_type: "image/svg+xml",
                body: vec![b' '; MAX_ICON_SVG_BYTES + 1],
            }
        } else {
            FakeResponse::status(404)
        }
    });
    let client = test_client();
    let url = format!("{}/mdi/cart.svg?height=256", server.base_url);
    let data_url = run(fetch_svg_data_url(&client, &url)).expect("embeds");
    assert!(data_url.starts_with("data:image/svg+xml;base64,"));
    assert!(run(fetch_svg_data_url(
        &client,
        &format!("{}/big/x.svg", server.base_url)
    ))
    .is_none());
    assert!(run(fetch_svg_data_url(
        &client,
        &format!("{}/missing.svg", server.base_url)
    ))
    .is_none());
}

#[test]
fn provider_search_keeps_only_licensed_icons() {
    let server = FakeHttp::start(|_, _| FakeResponse::json(search_answer()));
    let provider = ImageProvider::iconify_at(&server.base_url);
    assert!(!provider.needs_key());
    assert_eq!(provider.id(), "iconify");
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let hits = run(provider.search(&ctx, "cart", None)).unwrap();
    let ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    assert_eq!(ids, ["mdi:cart", "mdi:cart-outline"]);
    assert!(
        run(provider.test_connection(&ctx))
            == op_editor_core::agent_settings::ImageTestStatus::Valid
    );
}
