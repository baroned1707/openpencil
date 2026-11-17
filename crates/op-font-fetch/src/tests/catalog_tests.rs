use crate::catalog::{is_valid_catalog_id, normalize_family, Catalog};
use crate::fake::catalog_json;
use crate::license::License;
use crate::urls::{directory_token, font_file_url, is_safe_font_file_name};

#[test]
fn catalog_lookup_is_exact_and_case_insensitive() {
    let catalog = Catalog::parse(
        catalog_json(&[
            ("roboto-slab", "Roboto Slab", "Apache-2.0"),
            ("pacifico", "Pacifico", "OFL-1.1"),
        ])
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(catalog.lookup("roboto slab").unwrap().id, "roboto-slab");
    assert_eq!(
        catalog.lookup("  ROBOTO   Slab ").unwrap().id,
        "roboto-slab"
    );
    assert_eq!(catalog.lookup("\"Pacifico\"").unwrap().id, "pacifico");
    assert_eq!(catalog.lookup("'pacifico'").unwrap().id, "pacifico");
    // No prefix, substring or fuzzy matching.
    assert!(catalog.lookup("Roboto").is_none());
    assert!(catalog.lookup("Roboto Slab Pro").is_none());
    assert!(catalog.lookup("Pacific").is_none());
    assert!(catalog.lookup("").is_none());
}

#[test]
fn catalog_skips_hostile_and_non_google_entries_without_failing() {
    let json = serde_json::json!([
        {"id": "../../etc/passwd", "family": "Evil One", "license": "OFL-1.1", "type": "google"},
        {"id": "a/b", "family": "Evil Two", "license": "OFL-1.1", "type": "google"},
        {"id": "UPPER", "family": "Evil Three", "license": "OFL-1.1", "type": "google"},
        {"id": "", "family": "Evil Four", "license": "OFL-1.1", "type": "google"},
        {"id": "good-font", "family": "Good Font", "license": "OFL-1.1", "type": "google"},
        {"id": "other-font", "family": "Other Font", "license": "OFL-1.1", "type": "other"},
        {"id": 7, "family": "Numeric", "type": "google"},
        "not an object",
        {"family": "No Id", "type": "google"},
    ])
    .to_string();
    let catalog = Catalog::parse(json.as_bytes()).unwrap();
    assert_eq!(catalog.len(), 1);
    assert!(catalog.lookup("Good Font").is_some());
    for name in [
        "Evil One",
        "Evil Two",
        "Evil Three",
        "Evil Four",
        "Other Font",
    ] {
        assert!(catalog.lookup(name).is_none(), "{name} must be skipped");
    }
    assert!(Catalog::parse(b"{ not json").is_err());
    assert!(
        Catalog::parse(b"{\"a\":1}").is_err(),
        "an object is not the catalog"
    );
}

#[test]
fn only_the_three_open_licences_pass_the_allow_list() {
    for (raw, expected) in [
        ("OFL-1.1", Some(License::Ofl)),
        ("Apache-2.0", Some(License::Apache)),
        ("UFL-1.0", Some(License::Ufl)),
        ("CC0-1.0", None),
        ("mit", None),
        ("Unlicense", None),
        ("ofl-1.1", None),
        ("", None),
    ] {
        assert_eq!(License::from_catalog(raw), expected, "{raw}");
    }
    assert_eq!(License::from_metadata("OFL"), Some(License::Ofl));
    assert_eq!(License::from_metadata("APACHE2"), Some(License::Apache));
    assert_eq!(License::from_metadata("UFL"), Some(License::Ufl));
    assert_eq!(License::from_metadata("PROPRIETARY"), None);
}

#[test]
fn family_string_never_reaches_a_path_or_url() {
    // The URL helpers take a catalog id / file name, never a family. Ids that
    // are not `^[a-z0-9-]+$` yield no directory at all.
    let long = "a".repeat(300);
    let hostile = [
        "../../etc/passwd",
        "a/b",
        "evil\0name",
        long.as_str(),
        "R\u{043E}boto Slab",
        "\"Roboto Slab\"/../x",
        "roboto slab",
        "Roboto%2F..%2Fx",
        "a\\b",
        "",
    ];
    for name in hostile {
        assert_eq!(directory_token(name), None, "{name:?}");
    }
    assert_eq!(
        directory_token("roboto-slab").as_deref(),
        Some("robotoslab")
    );
    assert_eq!(directory_token("42dot-sans").as_deref(), Some("42dotsans"));
    assert!(!is_valid_catalog_id("Roboto"));

    // Control characters and absurd lengths do not even normalise.
    assert_eq!(normalize_family("evil\0name"), None);
    assert_eq!(normalize_family(&long), None);
    assert_eq!(normalize_family("   "), None);

    // File names from METADATA.pb are untrusted too.
    for name in [
        "../x.ttf",
        "a/b.ttf",
        "a\\b.ttf",
        ".hidden.ttf",
        "font.woff2",
        "font.ttf.exe",
        "font\0.ttf",
        "%2e%2e.ttf",
        "a b.ttf",
        ".ttf",
    ] {
        assert!(!is_safe_font_file_name(name), "{name:?}");
    }
    assert!(is_safe_font_file_name("Inter[opsz,wght].ttf"));
    assert!(is_safe_font_file_name("Pacifico-Regular.ttf"));

    // Brackets and commas are percent-encoded; the URL is exactly this.
    assert_eq!(
        font_file_url(License::Ofl, "inter", "Inter[opsz,wght].ttf"),
        "https://raw.githubusercontent.com/google/fonts/main/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf"
    );
}
