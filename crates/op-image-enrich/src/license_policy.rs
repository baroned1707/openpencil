//! Licence policy for catalogue results.
//!
//! Stock results are embedded in a design that the user can export, sell, and
//! modify, so a result is only usable when its licence allows commercial use
//! AND modification. The policy is a pure function of the provider's licence
//! short name and licence URL so it can be table-tested without a network.
//!
//! Decision table (any version of a licence family):
//!
//! | licence                          | policy            |
//! |----------------------------------|-------------------|
//! | CC0, PDM, public domain          | `Allow`           |
//! | CC BY                            | `AllowWithCredit` |
//! | CC BY-SA                         | `Refuse` (see below) unless share-alike is allowed |
//! | any NC variant, any ND variant   | `Refuse`          |
//! | empty / unknown / all rights reserved | `Refuse` (never guess) |
//!
//! Share-alike is refused by default because the SA terms can attach to the
//! exported design as a derivative work. [`ALLOW_SHARE_ALIKE_DEFAULT`] is the
//! single hook for relaxing that; there is no settings UI for it yet.

/// Default for the share-alike switch carried by `ProviderCtx`.
pub const ALLOW_SHARE_ALIKE_DEFAULT: bool = false;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicensePolicy {
    /// Usable with no credit line (public-domain dedications).
    Allow,
    /// Usable, but the design must credit the creator.
    AllowWithCredit,
    /// Not usable; the reason is for diagnostics only.
    Refuse(&'static str),
}

impl LicensePolicy {
    pub fn is_usable(&self) -> bool {
        !matches!(self, LicensePolicy::Refuse(_))
    }
}

/// [`license_policy_with`] with the default share-alike setting.
pub fn license_policy(license_short: &str, license_url: &str) -> LicensePolicy {
    license_policy_with(license_short, license_url, ALLOW_SHARE_ALIKE_DEFAULT)
}

/// Judge the short name and the URL independently and keep the stricter
/// answer, so a friendly name next to an NC URL (or the reverse) is refused.
pub fn license_policy_with(
    license_short: &str,
    license_url: &str,
    allow_share_alike: bool,
) -> LicensePolicy {
    let short = license_short.trim();
    let url = license_url.trim();
    if short.is_empty() && url.is_empty() {
        return LicensePolicy::Refuse("licence unknown");
    }
    let mut verdicts = Vec::new();
    if !short.is_empty() {
        verdicts.push(policy_for_tokens(
            &short_name_tokens(short),
            allow_share_alike,
        ));
    }
    if !url.is_empty() {
        verdicts.push(match url_license_code(url) {
            Some(code) => policy_for_tokens(&code, allow_share_alike),
            None => LicensePolicy::Refuse("licence URL not recognised"),
        });
    }
    // A refusal always wins; credit beats no-credit.
    if let Some(refusal) = verdicts
        .iter()
        .find(|verdict| matches!(verdict, LicensePolicy::Refuse(_)))
    {
        return refusal.clone();
    }
    if verdicts.contains(&LicensePolicy::AllowWithCredit) {
        LicensePolicy::AllowWithCredit
    } else {
        LicensePolicy::Allow
    }
}

fn policy_for_tokens(tokens: &[String], allow_share_alike: bool) -> LicensePolicy {
    let has = |wanted: &str| tokens.iter().any(|token| token == wanted);
    if has("nc") || has("noncommercial") || has("nd") || has("noderivatives") {
        return LicensePolicy::Refuse("non-commercial or no-derivatives licence");
    }
    if has("sa") || has("sharealike") {
        return if allow_share_alike {
            LicensePolicy::AllowWithCredit
        } else {
            LicensePolicy::Refuse("share-alike licence can attach to the exported design")
        };
    }
    if has("cc0") || has("pdm") || has("publicdomain") {
        return LicensePolicy::Allow;
    }
    if has("by") {
        return LicensePolicy::AllowWithCredit;
    }
    LicensePolicy::Refuse("licence not on the allow list")
}

/// Lower-case alphanumeric tokens of a short name ("CC BY-SA 4.0" →
/// `cc by sa 4 0`). "public domain" and "pd mark" collapse to one token so
/// the word "domain" alone never counts.
fn short_name_tokens(short: &str) -> Vec<String> {
    let lowered = short.to_ascii_lowercase();
    let mut tokens: Vec<String> = lowered
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect();
    let joined = tokens.join(" ");
    if joined.contains("public domain") || joined.contains("cc pd") {
        tokens.push("publicdomain".to_string());
    }
    if joined.contains("pd mark") {
        tokens.push("pdm".to_string());
    }
    tokens
}

/// The licence code carried by a Creative Commons URL:
/// `/licenses/by-nc-sa/4.0/` → `by nc sa`; `/publicdomain/zero/1.0/` → `cc0`;
/// `/publicdomain/mark/1.0/` → `pdm`. `None` for any other URL.
fn url_license_code(url: &str) -> Option<Vec<String>> {
    let lowered = url.to_ascii_lowercase();
    let path = lowered.split_once("creativecommons.org/")?.1;
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let family = segments.next()?;
    let code = segments.next()?;
    match (family, code) {
        ("publicdomain", "zero") => Some(vec!["cc0".to_string()]),
        ("publicdomain", "mark") => Some(vec!["pdm".to_string()]),
        ("licenses", code) => Some(
            code.split('-')
                .filter(|token| !token.is_empty())
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

/// Licence of every Pixabay hit (the service has one licence for all content).
pub const PIXABAY_LICENSE: &str = "Pixabay Content License";
pub const PIXABAY_LICENSE_URL: &str = "https://pixabay.com/service/license-summary/";
/// Licence of every Unsplash hit.
pub const UNSPLASH_LICENSE: &str = "Unsplash License";
pub const UNSPLASH_LICENSE_URL: &str = "https://unsplash.com/license";

/// Policy for a hit of a provider that reports a NAMED licence instead of a
/// Creative Commons one. Never guesses: a name the table does not know, or a
/// URL that is not the licence's own page, is refused.
///
/// | provider   | licence                                  | policy            |
/// |------------|------------------------------------------|-------------------|
/// | `pixabay`  | `Pixabay Content License` (own URL only) | `Allow`           |
/// | `unsplash` | `Unsplash License` (own URL only)        | `AllowWithCredit` |
/// | `iconify`  | an SPDX id, see [`iconify_spdx_policy`]  | per SPDX table    |
///
/// Any other provider falls through to [`license_policy_with`].
pub fn provider_license_policy(
    provider: &str,
    license_short: &str,
    license_url: &str,
    license_id: &str,
    allow_share_alike: bool,
) -> LicensePolicy {
    match provider {
        "pixabay" => named_license_policy(
            license_short,
            license_url,
            (PIXABAY_LICENSE, PIXABAY_LICENSE_URL),
            LicensePolicy::Allow,
        ),
        "unsplash" => named_license_policy(
            license_short,
            license_url,
            (UNSPLASH_LICENSE, UNSPLASH_LICENSE_URL),
            LicensePolicy::AllowWithCredit,
        ),
        "iconify" => iconify_spdx_policy(license_id, allow_share_alike),
        _ => license_policy_with(license_short, license_url, allow_share_alike),
    }
}

fn named_license_policy(
    license_short: &str,
    license_url: &str,
    (name, own_url): (&str, &str),
    verdict: LicensePolicy,
) -> LicensePolicy {
    if !license_short.trim().eq_ignore_ascii_case(name) {
        return LicensePolicy::Refuse("licence not on the allow list");
    }
    let url = license_url.trim().trim_end_matches('/');
    if !url.is_empty() && !url.eq_ignore_ascii_case(own_url.trim_end_matches('/')) {
        return LicensePolicy::Refuse("licence URL does not match the named licence");
    }
    verdict
}

/// Iconify reports one SPDX id per icon collection.
///
/// | SPDX id                                              | policy            |
/// |------------------------------------------------------|-------------------|
/// | `MIT`, `Apache-2.0`, `ISC`, `CC0-1.0`, `BSD-*`, `OFL-1.1` | `Allow`      |
/// | `CC-BY-4.0`, `CC-BY-3.0`                             | `AllowWithCredit` |
/// | `CC-BY-*-SA` (share-alike)                           | refused unless the hook allows it |
/// | any NC / ND variant, `GPL*`, `LGPL*`, `AGPL*`, empty, unknown | `Refuse` |
///
/// `Allow` here means "usable commercially"; the credit is still stored for
/// every hit because the licences above ask to keep the notice.
pub fn iconify_spdx_policy(spdx: &str, allow_share_alike: bool) -> LicensePolicy {
    let id = spdx.trim().to_ascii_lowercase();
    if id.is_empty() {
        return LicensePolicy::Refuse("licence unknown");
    }
    let tokens: Vec<&str> = id.split('-').collect();
    if tokens.iter().any(|token| matches!(*token, "nc" | "nd")) {
        return LicensePolicy::Refuse("non-commercial or no-derivatives licence");
    }
    if tokens.contains(&"sa") {
        return if allow_share_alike && id.starts_with("cc-by-") {
            LicensePolicy::AllowWithCredit
        } else {
            LicensePolicy::Refuse("share-alike licence can attach to the exported design")
        };
    }
    match id.as_str() {
        "mit" | "apache-2.0" | "isc" | "cc0-1.0" | "ofl-1.1" => LicensePolicy::Allow,
        "cc-by-4.0" | "cc-by-3.0" => LicensePolicy::AllowWithCredit,
        _ if id.starts_with("bsd-") => LicensePolicy::Allow,
        _ => LicensePolicy::Refuse("licence not on the allow list"),
    }
}
