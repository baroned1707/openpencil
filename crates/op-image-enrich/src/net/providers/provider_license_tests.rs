//! Named-licence policy of the key-based and icon providers.

use super::*;
use crate::license_policy::{
    PEXELS_LICENSE, PEXELS_LICENSE_URL, PIXABAY_LICENSE, PIXABAY_LICENSE_URL, UNSPLASH_LICENSE,
    UNSPLASH_LICENSE_URL,
};

fn refused(policy: &LicensePolicy) -> bool {
    matches!(policy, LicensePolicy::Refuse(_))
}

#[test]
fn license_policy_knows_named_provider_licenses() {
    // (provider, short name, url, expected)
    let table = [
        (
            "pixabay",
            PIXABAY_LICENSE,
            PIXABAY_LICENSE_URL,
            LicensePolicy::Allow,
        ),
        (
            "pixabay",
            "pixabay content license",
            "",
            LicensePolicy::Allow,
        ),
        (
            "unsplash",
            UNSPLASH_LICENSE,
            UNSPLASH_LICENSE_URL,
            LicensePolicy::AllowWithCredit,
        ),
        (
            "unsplash",
            UNSPLASH_LICENSE,
            "https://unsplash.com/license/",
            LicensePolicy::AllowWithCredit,
        ),
    ];
    for (provider, short, url, expected) in table {
        assert_eq!(
            provider_license_policy(provider, short, url, "", false),
            expected,
            "{provider} {short:?} {url:?}"
        );
    }

    // A licence the table does not know is refused, never guessed.
    for (provider, short, url) in [
        ("pixabay", "Some Other License", PIXABAY_LICENSE_URL),
        ("pixabay", "", ""),
        ("unsplash", PIXABAY_LICENSE, UNSPLASH_LICENSE_URL),
        ("unsplash", UNSPLASH_LICENSE, "https://example.com/license"),
        ("pixabay", PIXABAY_LICENSE, "https://evil.example/"),
    ] {
        assert!(
            refused(&provider_license_policy(provider, short, url, "", false)),
            "{provider} {short:?} {url:?} must be refused"
        );
    }

    // Named licences do not leak into the Creative Commons providers: the
    // same strings from Openverse or Commons stay refused, as before.
    assert!(refused(&license_policy(
        PIXABAY_LICENSE,
        PIXABAY_LICENSE_URL
    )));
    assert!(refused(&provider_license_policy(
        "wikimedia",
        UNSPLASH_LICENSE,
        UNSPLASH_LICENSE_URL,
        "",
        false
    )));
    assert!(refused(&provider_license_policy(
        "wikimedia",
        "MIT",
        "",
        "MIT",
        false
    )));
    assert_eq!(
        provider_license_policy("openverse", "CC BY 4.0", "", "", false),
        LicensePolicy::AllowWithCredit
    );
}

/// Invariant: a referral query on the licence link (Unsplash asks for
/// `utm_*` on every link back to it) does not change the verdict, but the
/// scheme, host and path must still be the licence's own page.
#[test]
fn named_license_url_tolerates_only_a_referral_query() {
    let utm = "?utm_source=openpencil&utm_medium=referral";
    for url in [
        UNSPLASH_LICENSE_URL.to_string(),
        format!("{UNSPLASH_LICENSE_URL}{utm}"),
        format!("{UNSPLASH_LICENSE_URL}/{utm}"),
    ] {
        assert_eq!(
            provider_license_policy("unsplash", UNSPLASH_LICENSE, &url, "", false),
            LicensePolicy::AllowWithCredit,
            "{url}"
        );
    }
    for url in [
        format!("https://evil.example/license{utm}"),
        format!("https://unsplash.com.evil.example/license{utm}"),
        format!("https://unsplash.com/other{utm}"),
        format!("http://unsplash.com/license{utm}"),
    ] {
        assert!(
            refused(&provider_license_policy(
                "unsplash",
                UNSPLASH_LICENSE,
                &url,
                "",
                false
            )),
            "{url} must be refused"
        );
    }
    // Pixabay keeps its own table: its URL with or without a query is fine,
    // another host is still refused.
    assert_eq!(
        provider_license_policy(
            "pixabay",
            PIXABAY_LICENSE,
            &format!("{PIXABAY_LICENSE_URL}{utm}"),
            "",
            false
        ),
        LicensePolicy::Allow
    );
    assert!(refused(&provider_license_policy(
        "pixabay",
        PIXABAY_LICENSE,
        &format!("https://evil.example/service/license-summary/{utm}"),
        "",
        false
    )));
}

/// Invariant: Pexels is a NAMED licence (usable, no credit required), never
/// read as CC0 and never accepted for another provider's hits.
#[test]
fn pexels_license_is_a_named_licence_not_cc0() {
    assert_eq!(
        provider_license_policy("pexels", PEXELS_LICENSE, PEXELS_LICENSE_URL, "", false),
        LicensePolicy::Allow
    );
    assert_eq!(
        provider_license_policy("pexels", "pexels license", "", "", false),
        LicensePolicy::Allow
    );
    for (short, url) in [
        ("CC0", PEXELS_LICENSE_URL),
        ("Public Domain", PEXELS_LICENSE_URL),
        (PEXELS_LICENSE, "https://evil.example/license/"),
        (PIXABAY_LICENSE, PEXELS_LICENSE_URL),
        ("", ""),
    ] {
        assert!(
            refused(&provider_license_policy("pexels", short, url, "", false)),
            "{short:?} {url:?} must be refused"
        );
    }
    // Not a Creative Commons licence: the CC path refuses it, as does any
    // other provider.
    assert!(refused(&license_policy(PEXELS_LICENSE, PEXELS_LICENSE_URL)));
    assert!(refused(&provider_license_policy(
        "openverse",
        PEXELS_LICENSE,
        PEXELS_LICENSE_URL,
        "",
        false
    )));
}

#[test]
fn iconify_license_allow_list_table() {
    use LicensePolicy::{Allow, AllowWithCredit};
    let allowed = [
        ("MIT", Allow),
        ("mit", Allow),
        ("Apache-2.0", Allow),
        ("ISC", Allow),
        ("CC0-1.0", Allow),
        ("BSD-2-Clause", Allow),
        ("BSD-3-Clause", Allow),
        ("OFL-1.1", Allow),
        ("CC-BY-4.0", AllowWithCredit),
        ("CC-BY-3.0", AllowWithCredit),
    ];
    for (spdx, expected) in allowed {
        assert_eq!(iconify_spdx_policy(spdx, false), expected, "{spdx}");
    }
    for spdx in [
        "CC-BY-NC-4.0",
        "CC-BY-NC-SA-4.0",
        "CC-BY-ND-4.0",
        "CC-BY-SA-4.0",
        "GPL-2.0",
        "GPL-3.0-only",
        "LGPL-3.0",
        "AGPL-3.0",
        "Unlicense",
        "CC-BY-2.0",
        "",
        "  ",
        "Custom-Terms",
    ] {
        assert!(refused(&iconify_spdx_policy(spdx, false)), "{spdx:?}");
    }
    // The share-alike hook relaxes CC BY-SA only, never NC / ND / GPL.
    assert_eq!(iconify_spdx_policy("CC-BY-SA-4.0", true), AllowWithCredit);
    assert!(refused(&iconify_spdx_policy("CC-BY-NC-SA-4.0", true)));
    assert!(refused(&iconify_spdx_policy("GPL-3.0-only", true)));
    // Through the hit gate the SPDX id decides, not the display title.
    let hit = |spdx: Option<&str>, title: &str| RawHit {
        provider: "iconify",
        license: Some(title.to_string()),
        license_id: spdx.map(str::to_string),
        license_url: Some("https://example.org/anything".to_string()),
        ..RawHit::default()
    };
    assert!(hit_license_is_usable(&hit(Some("MIT"), "MIT")));
    assert!(!hit_license_is_usable(&hit(None, "MIT")));
    assert!(!hit_license_is_usable(&hit(Some("GPL-3.0"), "MIT")));
}
