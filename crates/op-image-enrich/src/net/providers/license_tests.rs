use super::*;

const CC_BY_URL: &str = "https://creativecommons.org/licenses/by/4.0/";

#[test]
fn license_policy_table() {
    let allow = LicensePolicy::Allow;
    let credit = LicensePolicy::AllowWithCredit;
    // (short name, url, expected)
    let usable = [
        ("CC0", "", allow.clone()),
        (
            "CC0 1.0",
            "https://creativecommons.org/publicdomain/zero/1.0/",
            allow.clone(),
        ),
        ("PDM", "", allow.clone()),
        (
            "PDM 1.0",
            "https://creativecommons.org/publicdomain/mark/1.0/",
            allow.clone(),
        ),
        ("Public domain", "", allow.clone()),
        ("CC-PD-Mark", "", allow.clone()),
        ("CC BY 4.0", "", credit.clone()),
        ("CC BY 4.0", CC_BY_URL, credit.clone()),
        ("CC BY 2.0", "", credit.clone()),
        ("CC BY 3.0 DE", "", credit.clone()),
        ("", CC_BY_URL, credit.clone()),
    ];
    for (short, url, expected) in usable {
        assert_eq!(license_policy(short, url), expected, "{short:?} {url:?}");
    }
    let refused = [
        "CC BY-SA 4.0",
        "CC BY-SA",
        "CC BY-NC 2.0",
        "CC BY-NC-SA 4.0",
        "CC BY-ND 2.0",
        "CC BY-NC-ND 4.0",
        "All rights reserved",
        "Copyrighted free use",
        "GFDL",
        "sampling+",
        "",
    ];
    for short in refused {
        assert!(
            matches!(license_policy(short, ""), LicensePolicy::Refuse(_)),
            "{short:?} must be refused"
        );
    }
    assert!(matches!(license_policy("", ""), LicensePolicy::Refuse(_)));
}

#[test]
fn the_stricter_of_name_and_url_wins() {
    let nc_url = "https://creativecommons.org/licenses/by-nc/4.0/";
    assert!(matches!(
        license_policy("CC BY 4.0", nc_url),
        LicensePolicy::Refuse(_)
    ));
    assert!(matches!(
        license_policy("CC BY 4.0", "https://example.com/terms"),
        LicensePolicy::Refuse(_)
    ));
}

#[test]
fn share_alike_is_allowed_only_through_the_hook() {
    assert!(matches!(
        license_policy_with("CC BY-SA 4.0", "", false),
        LicensePolicy::Refuse(_)
    ));
    assert_eq!(
        license_policy_with("CC BY-SA 4.0", "", true),
        LicensePolicy::AllowWithCredit
    );
    // The hook never lifts the NC / ND refusals.
    assert!(matches!(
        license_policy_with("CC BY-NC-SA 4.0", "", true),
        LicensePolicy::Refuse(_)
    ));
}

#[test]
fn openverse_short_names_are_built_from_code_and_version() {
    assert_eq!(
        openverse_short_name("by", "4.0").as_deref(),
        Some("CC BY 4.0")
    );
    assert_eq!(
        openverse_short_name("by-sa", "2.0").as_deref(),
        Some("CC BY-SA 2.0")
    );
    assert_eq!(
        openverse_short_name("cc0", "1.0").as_deref(),
        Some("CC0 1.0")
    );
    assert_eq!(openverse_short_name("pdm", "").as_deref(), Some("PDM"));
    assert_eq!(openverse_short_name("", "4.0"), None);
}
