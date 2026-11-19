//! Where this fork publishes its releases, and whether it publishes any.
//!
//! Every URL the update path builds (release list, Atom feed, installer
//! download, the "open the project page" menu item) comes from the constants
//! here, so a fork can never reach another project's releases by accident.
//!
//! [`RELEASES_ENABLED`] stays `false` until the fork publishes signed
//! installers. While it is `false` the update probe makes no request and the
//! installer download is unreachable (see `update_check`).

/// GitHub owner of the fork's repository.
pub const RELEASE_OWNER: &str = "baroned1707";
/// GitHub repository name.
pub const RELEASE_REPO: &str = "openpencil";

/// Whether this build checks for and downloads releases at all.
///
/// `false` because the fork has published no signed release yet and its
/// repository is private (an unauthenticated API call would only answer 404).
/// Flipping it on also requires the download path to verify a signature
/// before it opens the installer; it currently does not.
pub const RELEASES_ENABLED: bool = false;

/// Project page, opened by the Help menu.
pub fn repo_url() -> String {
    format!("https://github.com/{RELEASE_OWNER}/{RELEASE_REPO}")
}

/// Releases page a found-update notice opens in the browser.
pub fn releases_url() -> String {
    format!("{}/releases", repo_url())
}

/// Releases list API endpoint. GitHub's `releases/latest` endpoint excludes
/// prereleases, so the updater uses the list feed and picks the first
/// non-draft release itself.
pub fn releases_api_url() -> String {
    format!("https://api.github.com/repos/{RELEASE_OWNER}/{RELEASE_REPO}/releases?per_page=20")
}

/// Releases Atom feed — the fallback when the JSON API is unavailable.
///
/// The anonymous API allows 60 requests per hour *per source IP*, so every
/// user behind a shared egress (corporate NAT, a VPN, most China-region
/// proxies) can find it already exhausted by strangers and see the probe fail
/// with "check the network" while the network is perfectly fine. The Atom
/// feed is served by github.com rather than api.github.com and is not on that
/// quota, so it answers when the API will not. Drafts never appear in it,
/// which is the same rule `select_latest_release_tag` applies to the API
/// response.
pub fn releases_atom_url() -> String {
    format!("{}/releases.atom", repo_url())
}

/// Whether `segment` is safe to place in a URL path unescaped. The version
/// comes from a remote release tag and the file name is derived from it, so
/// anything that could climb out of the release directory (`/`, `..`, a
/// percent-escape, a query or fragment) is refused instead of escaped.
fn is_plain_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Download URL of a release asset on `owner/repo`. `None` when `version` or
/// `name` is not a plain path segment. Pure so tests can pin it; the updater
/// always passes [`RELEASE_OWNER`] / [`RELEASE_REPO`].
pub fn installer_url(owner: &str, repo: &str, version: &str, name: &str) -> Option<String> {
    let plain = [owner, repo, version, name]
        .into_iter()
        .all(is_plain_path_segment);
    plain.then(|| format!("https://github.com/{owner}/{repo}/releases/download/v{version}/{name}"))
}

/// [`installer_url`] on the configured release repository.
pub fn configured_installer_url(version: &str, name: &str) -> Option<String> {
    installer_url(RELEASE_OWNER, RELEASE_REPO, version, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's names, spelled in pieces so this file never contains the
    /// literal and a repo-wide search for it stays meaningful.
    fn upstream_markers() -> [String; 2] {
        [["zseven", "-w"].concat(), ["zseven", "w"].concat()]
    }

    fn every_url() -> Vec<String> {
        vec![
            repo_url(),
            releases_url(),
            releases_api_url(),
            releases_atom_url(),
            configured_installer_url("0.9.0", "OpenPencil-0.9.0-arm64-mac.dmg").unwrap(),
        ]
    }

    #[test]
    fn no_configured_url_points_at_upstream() {
        for url in every_url() {
            let lower = url.to_ascii_lowercase();
            for marker in upstream_markers() {
                assert!(!lower.contains(&marker), "{url} must not name upstream");
            }
        }
    }

    #[test]
    fn urls_use_the_fork_owner_and_repo() {
        assert_eq!(repo_url(), "https://github.com/baroned1707/openpencil");
        assert_eq!(
            releases_api_url(),
            "https://api.github.com/repos/baroned1707/openpencil/releases?per_page=20"
        );
        assert_eq!(
            releases_atom_url(),
            "https://github.com/baroned1707/openpencil/releases.atom"
        );
        assert_eq!(
            configured_installer_url("0.9.0", "OpenPencil-0.9.0-x64-win-setup.exe").as_deref(),
            Some(
                "https://github.com/baroned1707/openpencil/releases/download/v0.9.0/\
                 OpenPencil-0.9.0-x64-win-setup.exe"
            )
        );
    }

    #[test]
    fn releases_stay_disabled_until_signed_releases_exist() {
        // Flipping this is a decision, not a refactor: the download path
        // opens an installer without verifying a signature.
        assert!(!RELEASES_ENABLED);
    }

    #[test]
    fn installer_url_refuses_a_version_or_name_that_escapes_the_release_path() {
        for bad in [
            "",
            ".",
            "..",
            "../../other/repo",
            "1.0/../x",
            "1.0?x=1",
            "1.0#frag",
            "1.0%2e%2e",
            "1 0",
        ] {
            assert_eq!(installer_url("o", "r", bad, "asset.dmg"), None, "{bad:?}");
            assert_eq!(installer_url("o", "r", "1.0.0", bad), None, "{bad:?}");
        }
        assert_eq!(installer_url("o/x", "r", "1.0.0", "a.dmg"), None);
        assert_eq!(installer_url("o", "r/x", "1.0.0", "a.dmg"), None);
        assert_eq!(
            installer_url("o", "r", "0.9.0-beta.1", "a.dmg").as_deref(),
            Some("https://github.com/o/r/releases/download/v0.9.0-beta.1/a.dmg")
        );
    }
}
