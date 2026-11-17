use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::TempDir;
use crate::catalog::CATALOG_URL;
use crate::catalog_cache::{CatalogCache, CatalogOrigin, CATALOG_TTL_SECS};
use crate::fake::{metadata_static, FakeFontSource, TTF_STUB};
use crate::fetch::{FamilyDownload, FetchFailure};
use crate::license::License;
use crate::plan::FaceRequest;
use crate::session::{FamilyStatus, FontFetcher, SessionConfig, StatusChange};
use crate::source::FetchError;

static CLOCK: AtomicU64 = AtomicU64::new(1_000_000);
fn clock() -> u64 {
    CLOCK.load(Ordering::SeqCst)
}

struct Rig {
    source: Arc<FakeFontSource>,
    fetcher: FontFetcher,
    _dir: TempDir,
}

const FAMILIES: [&str; 6] = [
    "Alpha Sans",
    "Beta Serif",
    "Gamma Mono",
    "Delta Script",
    "Epsilon Slab",
    "Zeta Round",
];

fn id_of(family: &str) -> String {
    family.to_lowercase().replace(' ', "-")
}

fn rig_with(config: SessionConfig) -> Rig {
    let dir = TempDir::new();
    let source = Arc::new(FakeFontSource::new());
    let entries: Vec<(String, &str, &str)> =
        FAMILIES.iter().map(|f| (id_of(f), *f, "OFL-1.1")).collect();
    let borrowed: Vec<(&str, &str, &str)> = entries
        .iter()
        .map(|(id, f, l)| (id.as_str(), *f, *l))
        .collect();
    source.serve_catalog(&borrowed);
    for family in FAMILIES {
        let file = format!("{}-Regular.ttf", family.replace(' ', ""));
        source.serve_family(
            &id_of(family),
            License::Ofl,
            &metadata_static(family, "OFL", &[(file.as_str(), "normal", 400)]),
            &[(file.as_str(), TTF_STUB)],
        );
    }
    let fetcher = FontFetcher::with_clock(
        source.clone(),
        CatalogCache::new(dir.0.join("catalog.json")),
        config,
        clock,
    );
    Rig {
        source,
        fetcher,
        _dir: dir,
    }
}

fn rig() -> Rig {
    rig_with(SessionConfig::default())
}

fn regular() -> BTreeSet<FaceRequest> {
    [FaceRequest {
        weight: 400,
        italic: false,
    }]
    .into_iter()
    .collect()
}

fn ok_register(_: &FamilyDownload) -> Result<(), FetchFailure> {
    Ok(())
}

/// Poll until `done` holds (or fail after 10 s), returning every change seen.
fn drive(
    fetcher: &mut FontFetcher,
    register: &mut dyn FnMut(&FamilyDownload) -> Result<(), FetchFailure>,
    done: impl Fn(&FontFetcher) -> bool,
) -> Vec<StatusChange> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        seen.extend(fetcher.poll(register));
        if done(fetcher) {
            return seen;
        }
        assert!(
            Instant::now() < deadline,
            "timed out; changes so far: {seen:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settle(fetcher: &mut FontFetcher) -> Vec<StatusChange> {
    drive(fetcher, &mut ok_register, |f| !f.is_busy())
}

#[test]
fn unknown_and_proprietary_families_are_never_fetched() {
    let mut rig = rig();
    for name in [
        "Helvetica Neue",
        "SF Pro",
        "Segoe UI",
        "Arial",
        "Not A Font",
    ] {
        rig.fetcher.request_download(name, regular(), false);
    }
    let changes = settle(&mut rig.fetcher);
    for name in [
        "Helvetica Neue",
        "SF Pro",
        "Segoe UI",
        "Arial",
        "Not A Font",
    ] {
        assert_eq!(
            rig.fetcher.status(name),
            Some(&FamilyStatus::Failed(FetchFailure::UnknownFont)),
            "{name}"
        );
    }
    assert!(changes
        .iter()
        .any(|(f, s)| f == "SF Pro" && *s == FamilyStatus::Failed(FetchFailure::UnknownFont)));
    // The catalog was fetched once; no METADATA or font URL was ever built.
    assert_eq!(rig.source.requests(), vec![CATALOG_URL.to_string()]);
}

#[test]
fn hostile_family_names_only_ever_cost_one_catalog_request() {
    let mut rig = rig();
    let long = "a".repeat(300);
    for name in [
        "../../etc/passwd",
        "a/b",
        "evil\0name",
        long.as_str(),
        "R\u{043E}boto",
        "\"Alpha Sans/../x\"",
    ] {
        rig.fetcher.request_download(name, regular(), false);
    }
    settle(&mut rig.fetcher);
    for url in rig.source.requests() {
        assert!(
            url == CATALOG_URL
                || url.starts_with("https://raw.githubusercontent.com/google/fonts/main/"),
            "unexpected URL {url}"
        );
        assert!(!url.contains("passwd") && !url.contains("evil") && !url.contains("aaaa"));
    }
    assert_eq!(rig.source.request_count(), 1);
}

#[test]
fn one_failure_does_not_stop_the_others() {
    let mut rig = rig();
    // Break Beta's font file only.
    rig.source.route(
        "https://raw.githubusercontent.com/google/fonts/main/ofl/betaserif/BetaSerif-Regular.ttf",
        Err(FetchError::Status(500)),
    );
    for name in ["Alpha Sans", "Beta Serif", "Gamma Mono"] {
        rig.fetcher.request_download(name, regular(), false);
    }
    let mut registered = Vec::new();
    drive(
        &mut rig.fetcher,
        &mut |d: &FamilyDownload| {
            registered.push(d.family.clone());
            Ok(())
        },
        |f| !f.is_busy(),
    );
    assert_eq!(
        rig.fetcher.status("Alpha Sans"),
        Some(&FamilyStatus::Downloaded)
    );
    assert_eq!(
        rig.fetcher.status("Gamma Mono"),
        Some(&FamilyStatus::Downloaded)
    );
    assert_eq!(
        rig.fetcher.status("Beta Serif"),
        Some(&FamilyStatus::Failed(FetchFailure::Http(500)))
    );
    registered.sort();
    assert_eq!(registered, ["Alpha Sans", "Gamma Mono"]);
}

#[test]
fn at_most_two_downloads_run_at_once() {
    let mut rig = rig();
    // Warm the catalog with the gate open so the workers do not queue up
    // behind the catalog request.
    rig.fetcher.request_download("Alpha Sans", regular(), false);
    settle(&mut rig.fetcher);

    rig.source.close_gate();
    for name in [
        "Beta Serif",
        "Gamma Mono",
        "Delta Script",
        "Epsilon Slab",
        "Zeta Round",
    ] {
        rig.fetcher.request_download(name, regular(), false);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while rig.source.active_now() < 2 {
        assert!(Instant::now() < deadline, "two workers never started");
        std::thread::sleep(Duration::from_millis(2));
    }
    // Give a (buggy) third worker time to show up.
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(rig.source.active_now(), 2);
    rig.source.open_gate();
    settle(&mut rig.fetcher);
    assert_eq!(rig.source.max_concurrent(), 2);
    for name in FAMILIES {
        assert_eq!(
            rig.fetcher.status(name),
            Some(&FamilyStatus::Downloaded),
            "{name}"
        );
    }
}

#[test]
fn failed_family_is_not_retried_until_user_clicks_retry() {
    let mut rig = rig();
    rig.source.set_offline(true);
    rig.fetcher.request_download("Alpha Sans", regular(), false);
    settle(&mut rig.fetcher);
    assert_eq!(
        rig.fetcher.status("Alpha Sans"),
        Some(&FamilyStatus::Failed(FetchFailure::Offline))
    );
    let after_first = rig.source.request_count();

    // Automatic re-requests (a re-detection of the same document) do nothing,
    // for this family and for another one: the catalog failure is cached too.
    assert!(!rig.fetcher.request_download("Alpha Sans", regular(), false));
    rig.fetcher.request_download("Beta Serif", regular(), false);
    settle(&mut rig.fetcher);
    assert_eq!(rig.source.request_count(), after_first);
    assert_eq!(
        rig.fetcher.status("Beta Serif"),
        Some(&FamilyStatus::Failed(FetchFailure::Offline))
    );

    // The user clicks Retry once the network is back.
    rig.source.set_offline(false);
    assert!(rig.fetcher.request_download("Alpha Sans", regular(), true));
    settle(&mut rig.fetcher);
    assert_eq!(
        rig.fetcher.status("Alpha Sans"),
        Some(&FamilyStatus::Downloaded)
    );
}

#[test]
fn non_retryable_failures_stay_failed_even_when_forced() {
    let mut rig = rig();
    rig.fetcher.request_download("SF Pro", regular(), false);
    settle(&mut rig.fetcher);
    let before = rig.source.request_count();
    assert!(!rig.fetcher.request_download("SF Pro", regular(), true));
    settle(&mut rig.fetcher);
    assert_eq!(rig.source.request_count(), before);
}

#[test]
fn a_registration_failure_marks_the_family_failed() {
    let mut rig = rig();
    rig.fetcher.request_download("Alpha Sans", regular(), false);
    drive(
        &mut rig.fetcher,
        &mut |_: &FamilyDownload| Err(FetchFailure::Invalid("does not parse".into())),
        |f| !f.is_busy(),
    );
    assert_eq!(
        rig.fetcher.status("Alpha Sans"),
        Some(&FamilyStatus::Failed(FetchFailure::Invalid(
            "does not parse".into()
        )))
    );
}

#[test]
fn the_session_download_budget_stops_further_families() {
    let mut rig = rig_with(SessionConfig {
        byte_budget: 1,
        ..SessionConfig::default()
    });
    rig.fetcher.request_download("Alpha Sans", regular(), false);
    settle(&mut rig.fetcher);
    assert!(rig.fetcher.bytes_downloaded() >= 1);
    rig.fetcher.request_download("Beta Serif", regular(), false);
    settle(&mut rig.fetcher);
    assert_eq!(
        rig.fetcher.status("Beta Serif"),
        Some(&FamilyStatus::Failed(FetchFailure::BudgetExceeded))
    );
}

#[test]
fn probe_uses_only_the_cache_and_never_the_network() {
    let mut rig = rig();
    // No cache yet: unknown downloadability, zero requests.
    rig.fetcher.probe("Helvetica Neue");
    settle(&mut rig.fetcher);
    assert_eq!(rig.source.request_count(), 0);
    assert_eq!(
        rig.fetcher.status("Helvetica Neue"),
        Some(&FamilyStatus::Ready)
    );

    // A download fills the cache; a NEW session then classifies from it.
    rig.fetcher.request_download("Alpha Sans", regular(), false);
    settle(&mut rig.fetcher);
    let cache_path = rig._dir.0.join("catalog.json");
    let requests_before = rig.source.request_count();
    let mut second = FontFetcher::with_clock(
        rig.source.clone(),
        CatalogCache::new(cache_path),
        SessionConfig::default(),
        clock,
    );
    second.probe("Helvetica Neue");
    second.probe("Alpha Sans");
    settle(&mut second);
    assert_eq!(
        second.status("Helvetica Neue"),
        Some(&FamilyStatus::Failed(FetchFailure::UnknownFont))
    );
    assert_eq!(second.status("Alpha Sans"), Some(&FamilyStatus::Ready));
    assert_eq!(rig.source.request_count(), requests_before);
}

#[test]
fn stale_catalog_cache_is_used_when_refresh_fails() {
    let dir = TempDir::new();
    let cache = CatalogCache::new(dir.0.join("catalog.json"));
    let source = FakeFontSource::new();
    source.serve_catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let t0 = 5_000_000;

    // First load: network, written to disk with the validator.
    let loaded = cache.load_or_refresh(&source, t0).unwrap();
    assert_eq!(loaded.origin, CatalogOrigin::Network);
    assert_eq!(loaded.etag.as_deref(), Some("\"fake-etag\""));
    assert_eq!(source.request_count(), 1);

    // Within 24 h: served from disk, no request.
    let loaded = cache
        .load_or_refresh(&source, t0 + CATALOG_TTL_SECS - 1)
        .unwrap();
    assert_eq!(loaded.origin, CatalogOrigin::FreshCache);
    assert_eq!(source.request_count(), 1);

    // Past 24 h and offline: the stale copy is still served.
    source.set_offline(true);
    let loaded = cache
        .load_or_refresh(&source, t0 + CATALOG_TTL_SECS + 1)
        .unwrap();
    assert_eq!(loaded.origin, CatalogOrigin::StaleCache);
    assert!(loaded.catalog.lookup("Pacifico").is_some());
    assert_eq!(source.request_count(), 2, "one failed refresh attempt");

    // A garbage refresh body also keeps the stale copy.
    source.set_offline(false);
    source.route_bytes(CATALOG_URL, b"<html>captive portal</html>");
    let loaded = cache
        .load_or_refresh(&source, t0 + CATALOG_TTL_SECS + 2)
        .unwrap();
    assert_eq!(loaded.origin, CatalogOrigin::StaleCache);

    // No cache and no network: the feature is unavailable.
    let empty = CatalogCache::new(dir.0.join("missing.json"));
    source.set_offline(true);
    assert!(empty.load_or_refresh(&source, t0).is_err());
    assert!(empty.load_cached(t0).is_none());
}
