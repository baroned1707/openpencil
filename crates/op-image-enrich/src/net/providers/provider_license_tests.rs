//! Named-licence policy of the key-based and icon providers.

use super::*;
use crate::license_policy::{
    PIXABAY_LICENSE, PIXABAY_LICENSE_URL, UNSPLASH_LICENSE, UNSPLASH_LICENSE_URL,
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
