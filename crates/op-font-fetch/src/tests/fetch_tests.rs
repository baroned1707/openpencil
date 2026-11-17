use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::catalog::Catalog;
use crate::fake::{catalog_json, metadata_static, metadata_variable, FakeFontSource, TTF_STUB};
use crate::fetch::{classify, fetch_family, FetchFailure, FetchLimits};
use crate::license::License;
use crate::plan::FaceRequest;
use crate::source::FetchError;
use crate::urls::{license_url, metadata_url};

fn limits() -> FetchLimits {
    FetchLimits {
        max_file_bytes: 1024,
        deadline: Instant::now() + Duration::from_secs(30),
    }
}

fn want(weight: u16) -> BTreeSet<FaceRequest> {
    [FaceRequest {
        weight,
        italic: false,
    }]
    .into_iter()
    .collect()
}

fn catalog(entries: &[(&str, &str, &str)]) -> Catalog {
    Catalog::parse(catalog_json(entries).as_bytes()).unwrap()
}

#[test]
fn disallowed_license_is_refused() {
    let cat = catalog(&[
        ("cc-font", "Cc Font", "CC0-1.0"),
        ("pacifico", "Pacifico", "OFL-1.1"),
    ]);
    assert_eq!(
        classify(&cat, "Cc Font").unwrap_err(),
        FetchFailure::LicenseNotAllowed
    );

    // The catalog says OFL but METADATA.pb says something else: refused too,
    // and no font file is ever requested.
    let source = FakeFontSource::new();
    source.serve_family(
        "pacifico",
        License::Ofl,
        &metadata_static(
            "Pacifico",
            "PROPRIETARY",
            &[("P-Regular.ttf", "normal", 400)],
        ),
        &[("P-Regular.ttf", TTF_STUB)],
    );
    let entry = classify(&cat, "Pacifico").unwrap();
    assert_eq!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::LicenseNotAllowed
    );
    assert!(source.requests().iter().all(|u| !u.ends_with(".ttf")));

    // Catalog and METADATA both allowed but disagreeing is also refused.
    let source = FakeFontSource::new();
    source.serve_family(
        "pacifico",
        License::Ofl,
        &metadata_static("Pacifico", "APACHE2", &[("P-Regular.ttf", "normal", 400)]),
        &[("P-Regular.ttf", TTF_STUB)],
    );
    assert_eq!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::LicenseNotAllowed
    );
}

#[test]
fn tries_ofl_then_apache_then_ufl() {
    let cat = catalog(&[("roboto-slab", "Roboto Slab", "Apache-2.0")]);
    let entry = classify(&cat, "Roboto Slab").unwrap();
    let source = FakeFontSource::new();
    source.serve_family(
        "roboto-slab",
        License::Apache,
        &metadata_variable(
            "Roboto Slab",
            "APACHE2",
            &[("RobotoSlab[wght].ttf", "normal")],
        ),
        &[("RobotoSlab[wght].ttf", TTF_STUB)],
    );
    let download = fetch_family(&source, entry, &want(700), &limits()).unwrap();
    let requests = source.requests();
    assert_eq!(requests[0], metadata_url(License::Ofl, "robotoslab"));
    assert_eq!(requests[1], metadata_url(License::Apache, "robotoslab"));
    assert_eq!(download.license, License::Apache);
    assert_eq!(download.license_file_name, "LICENSE.txt");
    assert!(requests.contains(&license_url(License::Apache, "robotoslab")));
    assert!(!requests.iter().any(|u| u.contains("/ufl/")));

    // 404 everywhere = not downloadable, after exactly three probes.
    let source = FakeFontSource::new();
    assert_eq!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::NotFound
    );
    assert_eq!(source.request_count(), 3);
    assert!(source.requests()[2].contains("/ufl/"));
}

#[test]
fn a_network_error_is_not_mistaken_for_a_missing_directory() {
    let cat = catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let entry = classify(&cat, "Pacifico").unwrap();
    let source = FakeFontSource::new();
    source.route(
        &metadata_url(License::Ofl, "pacifico"),
        Err(FetchError::Offline),
    );
    assert_eq!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::Offline
    );
    assert_eq!(
        source.request_count(),
        1,
        "no fall-through after a transport error"
    );
}

#[test]
fn non_ttf_and_oversized_files_are_rejected() {
    let cat = catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let entry = classify(&cat, "Pacifico").unwrap();
    let pb = metadata_static("Pacifico", "OFL", &[("P-Regular.ttf", "normal", 400)]);
    let serve = |bytes: &[u8]| {
        let source = FakeFontSource::new();
        source.serve_family("pacifico", License::Ofl, &pb, &[("P-Regular.ttf", bytes)]);
        source
    };
    // WOFF2, an HTML error page and OpenType-CFF are not TrueType.
    for bytes in [&b"wOF2\0\0\0\0"[..], b"<html>404</html>", b"OTTO\0\0\0\0"] {
        assert!(matches!(
            fetch_family(&serve(bytes), entry, &want(400), &limits()).unwrap_err(),
            FetchFailure::Invalid(_)
        ));
    }
    // Over the cap.
    let mut big = TTF_STUB.to_vec();
    big.resize(2048, 0);
    assert_eq!(
        fetch_family(&serve(&big), entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::TooLarge
    );
    // Within the cap it works.
    assert!(fetch_family(&serve(TTF_STUB), entry, &want(400), &limits()).is_ok());

    // A METADATA entry that names a non-.ttf file is never requested.
    let source = FakeFontSource::new();
    source.serve_family(
        "pacifico",
        License::Ofl,
        &metadata_static("Pacifico", "OFL", &[("P-Regular.woff2", "normal", 400)]),
        &[],
    );
    assert!(matches!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::Invalid(_)
    ));
    assert!(source.requests().iter().all(|u| !u.contains("woff2")));
}

#[test]
fn licence_text_comes_back_with_the_fonts() {
    let cat = catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let entry = classify(&cat, "Pacifico").unwrap();
    let source = FakeFontSource::new();
    source.serve_family(
        "pacifico",
        License::Ofl,
        &metadata_static("Pacifico", "OFL", &[("P-Regular.ttf", "normal", 400)]),
        &[("P-Regular.ttf", TTF_STUB)],
    );
    let download = fetch_family(&source, entry, &want(400), &limits()).unwrap();
    assert_eq!(download.license_file_name, "OFL.txt");
    assert_eq!(download.license_text, b"licence text");
    assert_eq!(download.files.len(), 1);
    assert_eq!(
        download.files[0].url,
        "https://raw.githubusercontent.com/google/fonts/main/ofl/pacifico/P-Regular.ttf"
    );
    assert_eq!(download.catalog_id, "pacifico");
}

#[test]
fn a_directory_holding_another_family_is_not_trusted() {
    let cat = catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let entry = classify(&cat, "Pacifico").unwrap();
    let source = FakeFontSource::new();
    source.serve_family(
        "pacifico",
        License::Ofl,
        &metadata_static("Something Else", "OFL", &[("X.ttf", "normal", 400)]),
        &[("X.ttf", TTF_STUB)],
    );
    assert_eq!(
        fetch_family(&source, entry, &want(400), &limits()).unwrap_err(),
        FetchFailure::NotFound
    );
}

#[test]
fn an_expired_family_deadline_stops_before_any_request() {
    let cat = catalog(&[("pacifico", "Pacifico", "OFL-1.1")]);
    let entry = classify(&cat, "Pacifico").unwrap();
    let source = FakeFontSource::new();
    let expired = FetchLimits {
        max_file_bytes: 1024,
        deadline: Instant::now(),
    };
    assert_eq!(
        fetch_family(&source, entry, &want(400), &expired).unwrap_err(),
        FetchFailure::Timeout
    );
    assert_eq!(source.request_count(), 0);
}
