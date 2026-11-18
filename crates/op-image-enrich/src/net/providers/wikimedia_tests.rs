//! Commons pacing against a loopback fake: serial requests, backoff on
//! 429/503, `Retry-After` precedence, and the skip-for-the-process rule.
//! Each test uses its own fake host, and pacing state is keyed by host, so
//! tests in this binary cannot poison each other.

use super::*;
use crate::net::providers::{ImageProvider, ProviderCtx};
use crate::net::test_support::{commons_reply, test_client, FakeHttp, FakeResponse};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn wikimedia_requests_are_serial_and_back_off_on_429() {
    // Serial: four concurrent searches never overlap at the host.
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let (live, peak) = (in_flight.clone(), max_in_flight.clone());
    let server = FakeHttp::start(move |_, _| {
        let now = live.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(now, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(40));
        live.fetch_sub(1, Ordering::SeqCst);
        commons_reply(Vec::new())
    });
    let base = server.base_url.clone();
    crate::net::block_on_image_runtime(async move {
        let handles: Vec<_> = (0..4)
            .map(|index| {
                let base = base.clone();
                tokio::spawn(async move {
                    fetch_pages_json(&test_client(), &base, &format!("query {index}"), 5).await
                })
            })
            .collect();
        for handle in handles {
            assert!(handle.await.unwrap().unwrap().is_some());
        }
    });
    assert_eq!(server.requests().len(), 4);
    assert_eq!(
        max_in_flight.load(Ordering::SeqCst),
        1,
        "requests overlapped"
    );

    // Exponential backoff: two 429s, then success. Gaps follow base * 2^n.
    let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let log = arrivals.clone();
    let server = FakeHttp::start(move |_, index| {
        log.lock().unwrap().push(Instant::now());
        if index < 2 {
            FakeResponse::status(429)
        } else {
            commons_reply(Vec::new())
        }
    });
    let policy = BackoffPolicy {
        base: Duration::from_millis(30),
        max: Duration::from_secs(1),
        retries: 2,
    };
    let reply = crate::net::block_on_image_runtime(fetch_pages_json_with(
        &test_client(),
        &server.base_url,
        "x",
        5,
        &policy,
    ));
    assert!(reply.unwrap().is_some(), "the third attempt succeeds");
    let times = arrivals.lock().unwrap().clone();
    assert_eq!(times.len(), 3);
    assert!(
        times[1] - times[0] >= Duration::from_millis(28),
        "first backoff"
    );
    assert!(
        times[2] - times[1] >= Duration::from_millis(58),
        "second backoff doubles"
    );

    // A 503 backs off too, and `Retry-After` beats a long exponential base.
    let server = FakeHttp::start(move |_, index| {
        if index == 0 {
            FakeResponse::status(503).with_header("Retry-After", "0")
        } else {
            commons_reply(Vec::new())
        }
    });
    let slow_base = BackoffPolicy {
        base: Duration::from_secs(30),
        max: Duration::from_secs(60),
        retries: 2,
    };
    let started = Instant::now();
    let reply = crate::net::block_on_image_runtime(fetch_pages_json_with(
        &test_client(),
        &server.base_url,
        "x",
        5,
        &slow_base,
    ));
    assert!(reply.unwrap().is_some());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "Retry-After: 0 ignored"
    );
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn repeated_429_skips_provider_for_the_process() {
    let server = FakeHttp::start(|_, _| FakeResponse::status(429).with_header("Retry-After", "0"));
    let client = test_client();
    let ctx = ProviderCtx::new(&client, None);
    let provider = ImageProvider::wikimedia_at(&server.base_url);

    let first = crate::net::block_on_image_runtime(provider.search(&ctx, "serum", None));
    assert_eq!(first.err(), Some(SearchError::Skipped));
    let sent = server.requests().len();
    assert_eq!(sent, 3, "three throttled answers in a row tip the provider");

    // From now on the provider is skipped without touching the network.
    let second = crate::net::block_on_image_runtime(provider.search(&ctx, "serum", None));
    assert_eq!(second.err(), Some(SearchError::Skipped));
    assert_eq!(server.requests().len(), sent);
    assert!(is_skipped(&server.base_url));
}

#[test]
fn commons_search_url_carries_the_requested_limit() {
    let url = search_url("https://commons.example", "kyoto temple", 10).expect("url");
    assert_eq!(url.path(), "/w/api.php");
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert!(pairs.contains(&("gsrlimit".into(), "10".into())));
    assert!(pairs.contains(&("gsrsearch".into(), "kyoto temple".into())));
}
