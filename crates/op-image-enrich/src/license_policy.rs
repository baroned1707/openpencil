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
