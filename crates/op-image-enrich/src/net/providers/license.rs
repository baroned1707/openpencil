//! Licence filtering of catalogue hits. The pure decision table lives in
//! [`crate::license_policy`] (no network dependency, shared with the credits
//! text); this module applies it to provider hits.

pub(crate) use crate::license_policy::ALLOW_SHARE_ALIKE_DEFAULT;
pub use crate::license_policy::{
    iconify_spdx_policy, license_policy, license_policy_with, provider_license_policy,
    LicensePolicy,
};

use super::catalog::RawHit;

/// The policy verdict for one hit: its provider's named-licence table when it
/// has one, the Creative Commons table otherwise.
pub(crate) fn hit_policy(hit: &RawHit, allow_share_alike: bool) -> LicensePolicy {
    provider_license_policy(
        hit.provider,
        hit.license.as_deref().unwrap_or(""),
        hit.license_url.as_deref().unwrap_or(""),
        hit.license_id.as_deref().unwrap_or(""),
        allow_share_alike,
    )
}

/// Whether one hit's licence is usable under the default policy.
pub(crate) fn hit_license_is_usable(hit: &RawHit) -> bool {
    hit_policy(hit, ALLOW_SHARE_ALIKE_DEFAULT).is_usable()
}

/// Drop every hit whose licence is not usable. Runs right after parsing and
/// before relevance filtering or judging, so a refused image can never be
/// ranked, judged, or chosen.
pub(crate) fn retain_usable_license(hits: Vec<RawHit>, allow_share_alike: bool) -> Vec<RawHit> {
    let total = hits.len();
    let kept: Vec<RawHit> = hits
        .into_iter()
        .filter(|hit| hit_policy(hit, allow_share_alike).is_usable())
        .collect();
    if kept.len() < total {
        eprintln!(
            "[ENRICH] licence: refused {} of {total} hits",
            total - kept.len()
        );
    }
    kept
}

/// `"CC BY 4.0"`-style short name from Openverse's separate `license`
/// (`by`, `by-sa`, `cc0`, `pdm`, ...) and `license_version` fields.
pub(crate) fn openverse_short_name(code: &str, version: &str) -> Option<String> {
    let code = code.trim().to_ascii_uppercase();
    if code.is_empty() {
        return None;
    }
    let version = version.trim();
    let name = if code == "CC0" || code == "PDM" {
        code
    } else {
        format!("CC {code}")
    };
    Some(if version.is_empty() {
        name
    } else {
        format!("{name} {version}")
    })
}

#[cfg(test)]
#[path = "license_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_license_tests.rs"]
mod provider_license_tests;
