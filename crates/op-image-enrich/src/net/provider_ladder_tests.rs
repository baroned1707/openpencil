//! Ladder and provider behaviour against a loopback fake (never the real
//! network): provider order, the two-keyword retry between providers, that
//! the popover / MCP / desktop ladders share one walk, and the licence gate.

use super::*;
use crate::net::resolve::resolve_thumbnail_over;
use crate::net::test_support::{
    commons_page, commons_reply, openverse_reply, openverse_result, query_param, test_client,
    FakeHttp, FakeResponse,
};

use std::collections::HashSet;
use std::sync::Mutex;

/// Long enough that the two-keyword retry has a shorter concrete tail.
const QUERY: &str = "minimal warm serum dropper bottle";

fn is_openverse(target: &str) -> bool {
    target.starts_with("/v1/images/")
}

fn is_commons(target: &str) -> bool {
    target.starts_with("/w/api.php")
}

/// One fake host answering both providers; `openverse_hits` / `commons_pages`
/// are built per request from the base URL the server learns lazily.
fn fake_catalogue(
    openverse_license: Option<&'static str>,
    commons_license: Option<(&'static str, &'static str)>,
) -> FakeHttp {
    let base = std::sync::Arc::new(Mutex::new(String::new()));
    let handler_base = base.clone();
    let server = FakeHttp::start(move |target, _| {
        let base = handler_base.lock().unwrap().clone();
        if target.starts_with("/thumb/") {
            FakeResponse::png()
        } else if is_openverse(target) {
            openverse_reply(match openverse_license {
                Some(license) => vec![openverse_result(
                    &base,
                    "ov1",
                    "Serum dropper bottle",
                    license,
                )],
                None => Vec::new(),
            })
        } else if is_commons(target) {
            commons_reply(match commons_license {
                Some((short, url)) => {
                    vec![commons_page(
                        &base,
                        7,
                        "File:Serum dropper bottle.png",
                        short,
                        url,
                    )]
                }
                None => Vec::new(),
            })
        } else {
            FakeResponse::status(404)
        }
    });
    *base.lock().unwrap() = server.base_url.clone();
    server
}

fn providers(server: &FakeHttp) -> Vec<ImageProvider> {
    vec![
        ImageProvider::openverse_at(&server.base_url),
        ImageProvider::wikimedia_at(&server.base_url),
    ]
}

fn catalogue_requests(server: &FakeHttp) -> Vec<String> {
    server
        .requests()
        .into_iter()
        .filter(|target| is_openverse(target) || is_commons(target))
        .collect()
}

fn kind_and_query(target: &str) -> (&'static str, String) {
    let kind = if is_openverse(target) {
        "openverse"
    } else {
        "wikimedia"
    };
    let key = if is_openverse(target) {
        "q"
    } else {
        "gsrsearch"
    };
    (kind, query_param(target, key).unwrap_or_default())
}

fn run_popover(server: &FakeHttp) -> WebImageSearchOutcome {
    let client = test_client();
    crate::net::block_on_image_runtime(run_search_over(
        &client,
        &providers(server),
        QUERY,
        None,
        |url: String| {
            let client = client.clone();
            async move { fetch_image_data_url(&client, &url).await }
        },
    ))
}

#[test]
fn ladder_order_is_unchanged_for_openverse_then_wikimedia() {
    // Openverse answers: Wikimedia must never be asked.
    let server = fake_catalogue(Some("by"), Some(("CC BY 4.0", "")));
    let outcome = run_popover(&server);
    assert_eq!(outcome.source, Some("openverse"));
    assert!(catalogue_requests(&server).iter().all(|t| is_openverse(t)));

    // Openverse empty: it is asked first, Wikimedia answers second.
    let server = fake_catalogue(None, Some(("CC BY 4.0", "")));
    let outcome = run_popover(&server);
    assert_eq!(outcome.source, Some("wikimedia"));
    let kinds: Vec<&str> = catalogue_requests(&server)
        .iter()
        .map(|target| kind_and_query(target).0)
        .collect();
    let first_commons = kinds.iter().position(|kind| *kind == "wikimedia").unwrap();
    assert!(first_commons > 0, "{kinds:?}");
    assert!(kinds[..first_commons]
        .iter()
        .all(|kind| *kind == "openverse"));
}

#[test]
fn two_keyword_retry_still_runs_between_providers() {
    let server = fake_catalogue(None, None);
    let outcome = run_popover(&server);
    assert_eq!(outcome.source, None);
    let primary = simplify_search_query(QUERY);
    let retry = two_keyword_retry(&primary).expect("the fixture query has a retry");
    let sequence: Vec<(&str, String)> = catalogue_requests(&server)
        .iter()
        .map(|target| kind_and_query(target))
        .collect();
    assert_eq!(
        sequence,
        vec![
            ("openverse", primary.clone()),
            ("openverse", retry.clone()),
            ("wikimedia", primary),
            ("wikimedia", retry),
        ]
    );
}

#[test]
fn mcp_enrich_and_desktop_use_the_same_ladder() {
    // The popover/daemon, MCP (`run_first_search_over`) and desktop unjudged
    // (`resolve_thumbnail_over`) entry points all walk `run_ladder`, so with
    // every list empty they send the identical request sequence.
    let sequence = |server: &FakeHttp| -> Vec<(&'static str, String)> {
        catalogue_requests(server)
            .iter()
            .map(|target| kind_and_query(target))
            .collect()
    };
    let client = test_client();

    let popover = fake_catalogue(None, None);
    run_popover(&popover);

    let mcp = fake_catalogue(None, None);
    crate::net::block_on_image_runtime(run_first_search_over(
        &client,
        &providers(&mcp),
        QUERY,
        None,
        |url: String| {
            let client = client.clone();
            async move { fetch_image_data_url(&client, &url).await }
        },
    ));

    let desktop = fake_catalogue(None, None);
    let used = Mutex::new(HashSet::new());
    let resolved = crate::net::block_on_image_runtime(resolve_thumbnail_over(
        &client,
        &providers(&desktop),
        QUERY,
        "",
        None,
        None,
        &used,
    ));
    assert!(resolved.is_none());

    assert_eq!(sequence(&popover).len(), 4);
    assert_eq!(sequence(&mcp), sequence(&popover));
    assert_eq!(sequence(&desktop), sequence(&popover));

    // And when Openverse has a usable hit, all three stop there.
    for run in 0..3 {
        let server = fake_catalogue(Some("cc0"), Some(("CC0", "")));
        match run {
            0 => {
                run_popover(&server);
            }
            1 => {
                crate::net::block_on_image_runtime(run_first_search_over(
                    &client,
                    &providers(&server),
                    QUERY,
                    None,
                    |url: String| {
                        let client = client.clone();
                        async move { fetch_image_data_url(&client, &url).await }
                    },
                ));
            }
            _ => {
                let resolved = crate::net::block_on_image_runtime(resolve_thumbnail_over(
                    &client,
                    &providers(&server),
                    QUERY,
                    "",
                    None,
                    None,
                    &Mutex::new(HashSet::new()),
                ))
                .expect("openverse hit resolves");
                assert_eq!(resolved.provider, "openverse");
                let provenance = resolved
                    .provenance
                    .expect("provenance travels with the hit");
                assert_eq!(provenance.provider, "openverse");
                assert_eq!(provenance.creator.as_deref(), Some("Jane Doe"));
            }
        }
        assert!(
            catalogue_requests(&server).iter().all(|t| is_openverse(t)),
            "run {run} reached Wikimedia"
        );
    }
}

#[test]
fn openverse_hit_carries_creator_license_and_source_url() {
    let server = fake_catalogue(Some("by"), None);
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let hits = crate::net::block_on_image_runtime(
        ImageProvider::openverse_at(&server.base_url).search(&ctx, "serum dropper bottle", None),
    )
    .expect("search");
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.provider, "openverse");
    assert_eq!(hit.creator.as_deref(), Some("Jane Doe"));
    assert_eq!(hit.license.as_deref(), Some("CC BY 4.0"));
    assert_eq!(
        hit.license_url.as_deref(),
        Some("https://creativecommons.org/licenses/by/4.0/")
    );
    assert_eq!(
        hit.source_page_url.as_deref(),
        Some("https://example.org/photo/ov1")
    );
    // The request asks the catalogue for commercially usable, modifiable works.
    let target = &server.requests()[0];
    assert_eq!(
        query_param(target, "license_type").as_deref(),
        Some("commercial,modification")
    );
}

#[test]
fn commons_artist_html_is_stripped() {
    let page = commons_page(
        "http://fake",
        7,
        "File:X.png",
        "CC BY-SA 4.0",
        "https://creativecommons.org/licenses/by-sa/4.0/",
    );
    let json = serde_json::json!({"query": {"pages": {"7": page}}});
    let hits = parse_wikimedia_results(&json);
    assert_eq!(hits[0].creator.as_deref(), Some("Jane & Co"));
    assert_eq!(hits[0].license.as_deref(), Some("CC BY-SA 4.0"));
    assert_eq!(
        hits[0].source_page_url.as_deref(),
        Some("https://commons.wikimedia.org/wiki/File:X.png")
    );
    assert_eq!(
        catalog::strip_html("A<br>B &lt;3 &quot;x&quot;<script>y</script>"),
        "A B <3 \"x\" y"
    );
}

#[test]
fn refused_license_never_reaches_relevance_or_judge() {
    // Provider level: an NC-only catalogue yields no hits at all, so neither
    // the fence nor a judge can ever see (or pick) it.
    let server = fake_catalogue(Some("by-nc"), Some(("CC BY-NC 2.0", "")));
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    for provider in providers(&server) {
        let hits =
            crate::net::block_on_image_runtime(provider.search(&ctx, "serum dropper bottle", None))
                .expect("search");
        assert!(
            hits.is_empty(),
            "{} leaked a refused licence",
            provider.id()
        );
    }
    // Ladder level: nothing is consumed and nothing is downloaded.
    let outcome = run_popover(&server);
    assert_eq!(outcome.source, None);
    assert!(server.requests().iter().all(|t| !t.starts_with("/thumb/")));
}

#[test]
fn search_test_button_resolves_valid_and_invalid() {
    use crate::net::providers::test_first_provider;
    use op_editor_core::agent_settings::ImageTestStatus;

    let client = test_client();
    let credentials = WebOpenverseCredentials::from_parts("id", "secret");

    // Valid: token granted, one-result query answered. The probe sends
    // exactly `q=test&page_size=1` with the token.
    let server = FakeHttp::start(|target, _| {
        if target.starts_with("/v1/auth_tokens/token/") {
            FakeResponse::json(serde_json::json!({"access_token": "tok", "expires_in": 3600}))
        } else {
            openverse_reply(Vec::new())
        }
    });
    let provider = vec![ImageProvider::openverse_at(&server.base_url)];
    let status = crate::net::block_on_image_runtime(test_first_provider(
        &client,
        &provider,
        credentials.as_ref(),
    ));
    assert_eq!(status, ImageTestStatus::Valid);
    let list = server
        .requests()
        .into_iter()
        .find(|target| is_openverse(target))
        .expect("list request");
    assert_eq!(query_param(&list, "q").as_deref(), Some("test"));
    assert_eq!(query_param(&list, "page_size").as_deref(), Some("1"));

    // Invalid: the token request is refused. The anonymous list would answer
    // 200 here, so Valid would mean the probe fell back to anonymous. A fresh
    // client id keeps the process-wide token cache of the case above out of it.
    let credentials = WebOpenverseCredentials::from_parts("wrong-id", "wrong-secret");
    let server = FakeHttp::start(|target, _| {
        if target.starts_with("/v1/auth_tokens/token/") {
            FakeResponse::status(401)
        } else {
            openverse_reply(Vec::new())
        }
    });
    let provider = vec![ImageProvider::openverse_at(&server.base_url)];
    let status = crate::net::block_on_image_runtime(test_first_provider(
        &client,
        &provider,
        credentials.as_ref(),
    ));
    assert_eq!(status, ImageTestStatus::Invalid);

    // Invalid: the catalogue itself errors.
    let server = FakeHttp::start(|_, _| FakeResponse::status(500));
    let provider = vec![ImageProvider::openverse_at(&server.base_url)];
    let status = crate::net::block_on_image_runtime(test_first_provider(&client, &provider, None));
    assert_eq!(status, ImageTestStatus::Invalid);

    // No provider at all is Invalid, never a hang.
    let status = crate::net::block_on_image_runtime(test_first_provider(&client, &[], None));
    assert_eq!(status, ImageTestStatus::Invalid);
}
