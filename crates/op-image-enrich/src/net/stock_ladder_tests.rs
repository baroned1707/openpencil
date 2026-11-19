//! The stock-provider ladders against ONE loopback host that answers every
//! provider's route: which providers a request walks, in which order, and what
//! each hit becomes. Nothing here reaches the real network.

use super::*;
use crate::net::resolve::resolve_thumbnail_over_kind;
use crate::net::test_support::{
    commons_page, commons_reply, openverse_reply, openverse_result, query_param, test_client,
    FakeHttp, FakeResponse,
};
use op_editor_core::ApiKey;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const QUERY: &str = "red bicycle";
const CART_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 24 24"><path fill="currentColor" d="M7 18c-1.1 0-2 .9-2 2z"/></svg>"#;

/// Which providers of the fake host have a result to give.
#[derive(Clone, Copy, Default)]
struct Has {
    pixabay: bool,
    unsplash: bool,
    pexels: bool,
    iconify: bool,
    openverse: bool,
    commons: bool,
}

fn route(target: &str) -> &'static str {
    if target.starts_with("/api/") {
        "pixabay"
    } else if target.starts_with("/search/photos") {
        "unsplash"
    } else if target.starts_with("/v1/search?") {
        "pexels"
    } else if target.starts_with("/search?") {
        "iconify"
    } else if target.starts_with("/v1/images/") {
        "openverse"
    } else if target.starts_with("/w/api.php") {
        "wikimedia"
    } else {
        "other"
    }
}

fn fake_host(has: Has) -> FakeHttp {
    let base = Arc::new(Mutex::new(String::new()));
    let handler_base = base.clone();
    let server = FakeHttp::start(move |target, _| {
        let base = handler_base.lock().unwrap().clone();
        match route(target) {
            "pixabay" => FakeResponse::json(serde_json::json!({
                "hits": if has.pixabay {
                    vec![serde_json::json!({
                        "id": 501,
                        "pageURL": "https://pixabay.com/photos/bike-501/",
                        "webformatURL": format!("{base}/thumb/501.png"),
                        "largeImageURL": format!("{base}/large/501.png"),
                        "user": "Pat", "tags": "red bicycle, street",
                    })]
                } else { Vec::new() }
            })),
            "unsplash" => FakeResponse::json(serde_json::json!({
                "results": if has.unsplash {
                    vec![serde_json::json!({
                        "id": "u1",
                        "alt_description": "a red bicycle on a street",
                        "urls": {"regular": format!("{base}/thumb/u1.png")},
                        "user": {"name": "Uma"},
                        "links": {
                            "html": "https://unsplash.com/photos/u1",
                            "download_location": format!("{base}/photos/u1/download"),
                        },
                    })]
                } else { Vec::new() }
            })),
            "pexels" => FakeResponse::json(serde_json::json!({
                "photos": if has.pexels {
                    vec![serde_json::json!({
                        "id": 9001,
                        "url": "https://www.pexels.com/photo/red-bicycle-9001/",
                        "photographer": "Pia",
                        "src": {
                            "large": format!("{base}/thumb/9001.png"),
                            "large2x": format!("{base}/thumb/9001.png"),
                        },
                        "alt": "a red bicycle on a street",
                    })]
                } else { Vec::new() }
            })),
            "iconify" => FakeResponse::json(if has.iconify {
                serde_json::json!({
                    "icons": ["mdi:bicycle"],
                    "collections": {"mdi": {
                        "name": "Material Design Icons",
                        "author": {"name": "Pictogrammers"},
                        "license": {"title": "Apache 2.0", "spdx": "Apache-2.0",
                                    "url": "https://example.org/license"},
                    }},
                })
            } else {
                serde_json::json!({"icons": [], "collections": {}})
            }),
            "openverse" => openverse_reply(if has.openverse {
                vec![openverse_result(&base, "ov1", "Red bicycle", "cc0")]
            } else {
                Vec::new()
            }),
            "wikimedia" => commons_reply(if has.commons {
                vec![commons_page(
                    &base,
                    7,
                    "File:Red bicycle.png",
                    "CC0",
                    "https://creativecommons.org/publicdomain/zero/1.0/",
                )]
            } else {
                Vec::new()
            }),
            _ if target.starts_with("/thumb/") => FakeResponse::png(),
            _ if target.ends_with(".svg") || target.contains(".svg?") => FakeResponse {
                status: 200,
                headers: Vec::new(),
                content_type: "image/svg+xml",
                body: CART_SVG.as_bytes().to_vec(),
            },
            _ => FakeResponse::status(404),
        }
    });
    *base.lock().unwrap() = server.base_url.clone();
    server
}

fn all_providers(server: &FakeHttp, kind: ImageKind) -> Vec<ImageProvider> {
    let key = |text: &str| ApiKey::new(text);
    let mut providers = Vec::new();
    match kind {
        ImageKind::Photo => {
            providers.push(ImageProvider::pixabay_at(&server.base_url, key("pk")));
            providers.push(ImageProvider::unsplash_at(&server.base_url, key("uk")));
        }
        ImageKind::Vector => providers.push(ImageProvider::iconify_at(&server.base_url)),
    }
    providers.push(ImageProvider::openverse_at(&server.base_url));
    providers.push(ImageProvider::wikimedia_at(&server.base_url));
    providers
}

fn walked(server: &FakeHttp) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for target in server.requests() {
        let stage = route(&target);
        if stage != "other" && seen.last() != Some(&stage) {
            seen.push(stage);
        }
    }
    seen
}

fn resolve(
    server: &FakeHttp,
    kind: ImageKind,
    intent: &str,
) -> Option<crate::net::resolve::ResolvedImage> {
    let client = test_client();
    crate::net::block_on_image_runtime(resolve_thumbnail_over_kind(
        &client,
        &all_providers(server, kind),
        kind,
        QUERY,
        intent,
        None,
        None,
        &Mutex::new(HashSet::new()),
    ))
}

fn ids(providers: &[ImageProvider]) -> Vec<&'static str> {
    providers.iter().map(ImageProvider::id).collect()
}

#[test]
fn ladder_without_keys_is_unchanged() {
    let today = vec![ImageProvider::openverse(), ImageProvider::wikimedia()];
    assert_eq!(default_providers(), today);
    assert_eq!(
        providers_for(ImageKind::Photo, &StockKeys::default()),
        today
    );
    // A key that is only whitespace is no key; an Unsplash key without the
    // opt-in is not enough; the opt-in without a key is not enough either.
    for keys in [
        StockKeys {
            pixabay: ApiKey::new("   "),
            ..StockKeys::default()
        },
        StockKeys {
            unsplash: ApiKey::new("uk"),
            unsplash_enabled: false,
            ..StockKeys::default()
        },
        StockKeys {
            unsplash_enabled: true,
            ..StockKeys::default()
        },
    ] {
        assert_eq!(providers_for(ImageKind::Photo, &keys), today, "{keys:?}");
    }
}

#[test]
fn photo_ladder_prefers_pixabay_then_unsplash_then_openverse_then_wikimedia() {
    let keys = StockKeys {
        pixabay: ApiKey::new("pk"),
        unsplash: ApiKey::new("uk"),
        unsplash_enabled: true,
        ..StockKeys::default()
    };
    assert_eq!(
        ids(&providers_for(ImageKind::Photo, &keys)),
        ["pixabay", "unsplash", "openverse", "wikimedia"]
    );

    // Each provider answers when everything before it is empty, and the walk
    // stops at the first hit.
    let cases = [
        (
            Has {
                pixabay: true,
                unsplash: true,
                openverse: true,
                commons: true,
                ..Has::default()
            },
            "pixabay",
            vec!["pixabay"],
        ),
        (
            Has {
                unsplash: true,
                openverse: true,
                commons: true,
                ..Has::default()
            },
            "unsplash",
            vec!["pixabay", "unsplash"],
        ),
        (
            Has {
                openverse: true,
                commons: true,
                ..Has::default()
            },
            "openverse",
            vec!["pixabay", "unsplash", "openverse"],
        ),
        (
            Has {
                commons: true,
                ..Has::default()
            },
            "wikimedia",
            vec!["pixabay", "unsplash", "openverse", "wikimedia"],
        ),
    ];
    for (has, expected_provider, expected_walk) in cases {
        let server = fake_host(has);
        let resolved = resolve(&server, ImageKind::Photo, "").expect("a provider answers");
        assert_eq!(resolved.provider, expected_provider);
        assert_eq!(walked(&server), expected_walk, "{expected_provider}");
        let credit = resolved.provenance.expect("every hit carries a credit");
        assert_eq!(credit.provider, expected_provider);
    }
}

#[test]
fn unsplash_needs_key_and_optin() {
    let matrix = [
        // (key, enabled, runs)
        ("uk", true, true),
        ("uk", false, false),
        ("", true, false),
        ("  ", true, false),
        ("", false, false),
    ];
    for (key, enabled, runs) in matrix {
        let keys = StockKeys {
            unsplash: ApiKey::new(key),
            unsplash_enabled: enabled,
            ..StockKeys::default()
        };
        let listed = ids(&providers_for(ImageKind::Photo, &keys));
        assert_eq!(
            listed.contains(&"unsplash"),
            runs,
            "key={key:?} on={enabled}"
        );
        // The Vector ladder never takes a stock key provider.
        assert!(!ids(&providers_for(ImageKind::Vector, &keys)).contains(&"unsplash"));
    }
    let provider = ImageProvider::unsplash(ApiKey::new("uk"));
    assert!(provider.needs_key());
    assert!(!StockKeys::default().unsplash_enabled);
}

#[test]
fn vector_kind_uses_iconify_then_openverse_svg_then_commons() {
    assert_eq!(
        ids(&providers_for(ImageKind::Vector, &StockKeys::default())),
        ["iconify", "openverse", "wikimedia"]
    );

    // Nothing at Iconify or Openverse: Commons answers, and both fallbacks
    // were asked for SVG only.
    let server = fake_host(Has {
        commons: true,
        ..Has::default()
    });
    let resolved = resolve(&server, ImageKind::Vector, "").expect("commons answers");
    assert_eq!(resolved.provider, "wikimedia");
    assert_eq!(walked(&server), ["iconify", "openverse", "wikimedia"]);
    let requests = server.requests();
    let openverse = requests.iter().find(|t| route(t) == "openverse").unwrap();
    assert_eq!(query_param(openverse, "extension").as_deref(), Some("svg"));
    let commons = requests.iter().find(|t| route(t) == "wikimedia").unwrap();
    assert!(query_param(commons, "gsrsearch")
        .unwrap()
        .ends_with("filetype:drawing"));

    // An Iconify hit embeds as an SVG data URL, credited to its collection,
    // even for a slot whose intent carries a photo contract.
    let server = fake_host(Has {
        iconify: true,
        openverse: true,
        ..Has::default()
    });
    let resolved = resolve(&server, ImageKind::Vector, "product photo of a red bicycle")
        .expect("iconify answers");
    assert_eq!(resolved.provider, "iconify");
    assert!(resolved.src.starts_with("data:image/svg+xml;base64,"));
    assert_eq!(walked(&server), ["iconify"]);
    let credit = resolved.provenance.unwrap();
    assert_eq!(credit.provider, "iconify");
    assert_eq!(credit.license.as_deref(), Some("Apache 2.0"));
    assert_eq!(credit.creator.as_deref(), Some("Pictogrammers"));

    // The same Openverse request in a PHOTO ladder carries no `extension`.
    let photo = fake_host(Has {
        openverse: true,
        ..Has::default()
    });
    resolve(&photo, ImageKind::Photo, "").unwrap();
    let requests = photo.requests();
    let openverse = requests.iter().find(|t| route(t) == "openverse").unwrap();
    assert!(query_param(openverse, "extension").is_none());
}

#[test]
fn a_provider_that_errors_hands_over_to_the_next() {
    // Pixabay answers HTTP 500: the walk continues to Openverse.
    let base = Arc::new(Mutex::new(String::new()));
    let handler_base = base.clone();
    let server = FakeHttp::start(move |target, _| {
        let base = handler_base.lock().unwrap().clone();
        match route(target) {
            "pixabay" => FakeResponse::status(500),
            "openverse" => {
                openverse_reply(vec![openverse_result(&base, "ov1", "Red bicycle", "cc0")])
            }
            _ if target.starts_with("/thumb/") => FakeResponse::png(),
            _ => FakeResponse::status(404),
        }
    });
    *base.lock().unwrap() = server.base_url.clone();
    let resolved = resolve(&server, ImageKind::Photo, "").expect("openverse answers");
    assert_eq!(resolved.provider, "openverse");
    assert_eq!(walked(&server), ["pixabay", "unsplash", "openverse"]);
}

#[test]
fn per_provider_test_button_resolves_valid_and_invalid() {
    use op_editor_core::agent_settings::ImageTestStatus::{Invalid, Valid};
    let client = test_client();
    let run = |provider: ImageProvider| {
        let client = client.clone();
        crate::net::block_on_image_runtime(async move {
            let ctx = ProviderCtx::new(&client, None);
            provider.test_connection(&ctx).await
        })
    };
    // A healthy host: Pixabay, Unsplash and Iconify each answer Valid.
    let healthy = fake_host(Has::default());
    assert_eq!(
        run(ImageProvider::pixabay_at(
            &healthy.base_url,
            ApiKey::new("pk")
        )),
        Valid
    );
    assert_eq!(
        run(ImageProvider::unsplash_at(
            &healthy.base_url,
            ApiKey::new("uk")
        )),
        Valid
    );
    assert_eq!(
        run(ImageProvider::pexels_at(
            &healthy.base_url,
            ApiKey::new("xk")
        )),
        Valid
    );
    assert_eq!(run(ImageProvider::iconify_at(&healthy.base_url)), Valid);
    // A host that refuses the key (Pixabay answers 400, Unsplash 401).
    let refusing = FakeHttp::start(|target, _| match route(target) {
        "pixabay" => FakeResponse::status(400),
        _ => FakeResponse::status(401),
    });
    assert_eq!(
        run(ImageProvider::pixabay_at(
            &refusing.base_url,
            ApiKey::new("bad")
        )),
        Invalid
    );
    assert_eq!(
        run(ImageProvider::unsplash_at(
            &refusing.base_url,
            ApiKey::new("bad")
        )),
        Invalid
    );
    assert_eq!(
        run(ImageProvider::pexels_at(
            &refusing.base_url,
            ApiKey::new("bad")
        )),
        Invalid
    );
    // An empty key never dials.
    let never = fake_host(Has::default());
    assert_eq!(
        run(ImageProvider::pixabay_at(&never.base_url, ApiKey::new(""))),
        Invalid
    );
    assert!(never.requests().is_empty());
}

/// A judge that rates every candidate On, in provider order.
struct AlwaysOn;

impl crate::net::ImageRelevanceJudge for AlwaysOn {
    fn judge(&self, _: &str, _: &str, thumbs: &[Vec<u8>]) -> Vec<crate::net::JudgedCandidate> {
        (0..thumbs.len())
            .map(|index| crate::net::JudgedCandidate {
                index,
                verdict: crate::net::RelevanceVerdict::On,
                reason: "scripted".into(),
            })
            .collect()
    }
}

#[test]
fn judged_ladder_asks_pixabay_before_the_open_catalogues() {
    // The judged ladder keeps its own Openverse / Commons stages; the
    // key-based stock stage runs first. With a hit judged On it ends there,
    // so the real Openverse and Commons hosts are never contacted.
    let server = fake_host(Has {
        pixabay: true,
        ..Has::default()
    });
    let providers = [ImageProvider::pixabay_at(
        &server.base_url,
        ApiKey::new("pk"),
    )];
    let used = Mutex::new(HashSet::new());
    let (resolved, _) =
        crate::net::block_on_image_runtime(crate::net::fetch::fetch_first_image_url_with_judge(
            QUERY, None, None, &used, &AlwaysOn, "", None, None, &providers,
        ))
        .expect("the judged Pixabay hit wins");
    assert_eq!(resolved.provider, "pixabay");
    assert_eq!(walked(&server), ["pixabay"]);
    let credit = resolved.provenance.expect("credit travels with the hit");
    assert_eq!(credit.provider, "pixabay");
    assert_eq!(credit.creator.as_deref(), Some("Pat"));
    assert!(resolved.src.starts_with("data:image/"));
}

/// Invariant: Pexels runs only with a key AND its switch, and sits between
/// Unsplash and Openverse. Off (the default), keyless or whitespace-only it is
/// absent, and it never joins the Vector ladder.
#[test]
fn pexels_needs_key_and_optin_and_sits_between_unsplash_and_openverse() {
    let matrix = [
        // (key, enabled, runs)
        ("xk", true, true),
        ("xk", false, false),
        ("", true, false),
        ("  ", true, false),
        ("", false, false),
    ];
    for (key, enabled, runs) in matrix {
        let keys = StockKeys {
            pexels: ApiKey::new(key),
            pexels_enabled: enabled,
            ..StockKeys::default()
        };
        let listed = ids(&providers_for(ImageKind::Photo, &keys));
        assert_eq!(listed.contains(&"pexels"), runs, "key={key:?} on={enabled}");
        assert!(!ids(&providers_for(ImageKind::Vector, &keys)).contains(&"pexels"));
        if !runs {
            assert_eq!(listed, ["openverse", "wikimedia"]);
        }
    }
    let everything = StockKeys {
        pixabay: ApiKey::new("pk"),
        unsplash: ApiKey::new("uk"),
        unsplash_enabled: true,
        pexels: ApiKey::new("xk"),
        pexels_enabled: true,
    };
    assert_eq!(
        ids(&providers_for(ImageKind::Photo, &everything)),
        ["pixabay", "unsplash", "pexels", "openverse", "wikimedia"]
    );
    // A key with the switch off is skipped without disturbing the others.
    let off = StockKeys {
        pexels_enabled: false,
        ..everything.clone()
    };
    assert_eq!(
        ids(&providers_for(ImageKind::Photo, &off)),
        ["pixabay", "unsplash", "openverse", "wikimedia"]
    );
    assert!(ImageProvider::pexels(ApiKey::new("xk")).needs_key());
    assert!(!StockKeys::default().pexels_enabled);
}

fn pexels_walk() -> impl Fn(&FakeHttp) -> Vec<ImageProvider> {
    |server| {
        vec![
            ImageProvider::pixabay_at(&server.base_url, ApiKey::new("pk")),
            ImageProvider::unsplash_at(&server.base_url, ApiKey::new("uk")),
            ImageProvider::pexels_at(&server.base_url, ApiKey::new("xk")),
            ImageProvider::openverse_at(&server.base_url),
            ImageProvider::wikimedia_at(&server.base_url),
        ]
    }
}

#[test]
fn pexels_answers_after_pixabay_and_unsplash_and_before_openverse() {
    let providers_of = pexels_walk();
    let client = test_client();
    let resolve_over = |server: &FakeHttp| {
        crate::net::block_on_image_runtime(resolve_thumbnail_over_kind(
            &client,
            &providers_of(server),
            ImageKind::Photo,
            QUERY,
            "",
            None,
            None,
            &Mutex::new(HashSet::new()),
        ))
    };
    let cases = [
        (
            Has {
                pexels: true,
                openverse: true,
                commons: true,
                ..Has::default()
            },
            "pexels",
            vec!["pixabay", "unsplash", "pexels"],
        ),
        (
            Has {
                unsplash: true,
                pexels: true,
                ..Has::default()
            },
            "unsplash",
            vec!["pixabay", "unsplash"],
        ),
        (
            Has {
                openverse: true,
                commons: true,
                ..Has::default()
            },
            "openverse",
            vec!["pixabay", "unsplash", "pexels", "openverse"],
        ),
    ];
    for (has, expected_provider, expected_walk) in cases {
        let server = fake_host(has);
        let resolved = resolve_over(&server).expect("a provider answers");
        assert_eq!(resolved.provider, expected_provider);
        assert_eq!(walked(&server), expected_walk, "{expected_provider}");
        let credit = resolved.provenance.expect("every hit carries a credit");
        assert_eq!(credit.provider, expected_provider);
        if expected_provider == "pexels" {
            assert_eq!(credit.creator.as_deref(), Some("Pia"));
            assert_eq!(credit.license.as_deref(), Some("Pexels License"));
            assert_eq!(
                credit.source_page_url.as_deref(),
                Some("https://www.pexels.com/photo/red-bicycle-9001/")
            );
            assert!(resolved.download_ping.is_none());
        }
    }
}
