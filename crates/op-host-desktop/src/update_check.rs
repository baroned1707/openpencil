//! Background auto-update check.
//!
//! The TS Electron app ships a full `electron-updater` pipeline
//! (download + signature-verify + self-install). The Rust desktop
//! host has no signed-installer pipeline yet, so this module does
//! the part that is safe and useful without one: a background probe
//! of the fork's GitHub releases API that compares the running build to
//! the latest published release and surfaces the result.
//!
//! Flow: [`UpdateProbe::spawn`] runs one HTTPS request on a worker
//! thread (never the UI thread); the runner drains the outcome with
//! [`UpdateProbe::poll`] and writes it into
//! `EditorUiState::update_status` (rendered by the settings System
//! tab). When a newer release is found the runner offers to open
//! the download page via [`open_url`].
//!
//! While [`release_channel::RELEASES_ENABLED`] is `false` (this fork has no
//! signed releases yet) the probe makes no request and reports
//! `UpdateStatus::Unavailable`, and the installer download never runs.
//!
//! Every failure mode (offline, rate-limited, malformed JSON)
//! degrades to `UpdateStatus::Error` — the probe never panics and
//! never blocks shutdown.

use std::sync::mpsc::{self, Receiver, TryRecvError};

use op_editor_core::UpdateStatus;

pub mod release_channel;

use release_channel::RELEASES_ENABLED;

/// Releases page a found-update notice opens in the browser.
pub fn releases_url() -> String {
    release_channel::releases_url()
}

/// Background release-API probe. One request per `spawn`; the
/// runner re-spawns to re-check (e.g. from the "Check for Updates"
/// menu item).
pub struct UpdateProbe {
    rx: Option<Receiver<UpdateStatus>>,
}

impl UpdateProbe {
    /// Startup probe constructor that honors the persisted
    /// auto-update preference.
    pub fn for_auto_check(auto_update_enabled: bool) -> Self {
        if auto_update_enabled {
            Self::spawn()
        } else {
            Self::idle()
        }
    }

    /// Disabled auto-check state. Manual checks still call
    /// [`UpdateProbe::spawn`] directly.
    pub fn idle() -> Self {
        Self { rx: None }
    }

    /// Spawn the probe worker. Returns immediately; the request
    /// runs on its own thread. With releases disabled no request is made
    /// and the probe resolves to `Unavailable`.
    pub fn spawn() -> Self {
        Self::spawn_with(RELEASES_ENABLED, check_latest_release)
    }

    /// [`UpdateProbe::spawn`] with the release switch and the check injected.
    /// A disabled probe never runs `check`: its result is ready at once.
    fn spawn_with(
        releases_enabled: bool,
        check: impl FnOnce() -> UpdateStatus + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        if releases_enabled {
            std::thread::spawn(move || {
                let _ = tx.send(check());
            });
        } else {
            let _ = tx.send(UpdateStatus::Unavailable);
        }
        Self { rx: Some(rx) }
    }

    /// Whether the probe worker is still running (result not yet
    /// drained). The runner uses this to keep waking the event loop
    /// so a result that lands while the app is idle is still drained.
    pub fn is_pending(&self) -> bool {
        self.rx.is_some()
    }

    /// Drain the probe result if it has landed. Returns `Some` once
    /// (when the worker resolves), `None` while still in flight or
    /// after it has already been drained — so the runner can write
    /// the new status and act on a found update exactly once.
    pub fn poll(&mut self) -> Option<UpdateStatus> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(status) => {
                self.rx = None;
                Some(status)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                Some(UpdateStatus::Error)
            }
        }
    }
}

/// Query the releases list and classify the running build against the newest
/// non-draft release, including prereleases. Blocking — only ever called on the
/// probe worker thread.
fn check_latest_release() -> UpdateStatus {
    let Some(tag) = fetch_latest_tag() else {
        return UpdateStatus::Error;
    };
    let latest = tag.trim().trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");
    if is_newer(latest, current) {
        UpdateStatus::Available {
            version: latest.to_string(),
        }
    } else {
        UpdateStatus::UpToDate
    }
}

/// Run the HTTPS request. `reqwest` is async-only in this
/// workspace (no `blocking` feature), so the worker bridges onto the
/// shared runtime for the one call — a private per-call runtime aborts
/// with "Cannot start a runtime from within a runtime" whenever this
/// probe is reached from a tokio worker. `None` on any transport /
/// parse failure.
fn fetch_latest_tag() -> Option<String> {
    op_host_services::chat_runtime::block_on_anywhere(async {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(std::time::Duration::from_secs(15))
            // GitHub's API rejects requests without a User-Agent.
            .user_agent(concat!("openpencil-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        // The API carries the richer response, so it stays the first choice;
        // the feed only has to cover the case where the API refuses to answer
        // at all. A rate-limited probe must not read as "you are offline".
        if let Some(tag) = fetch_tag_from_api(&client).await {
            return Some(tag);
        }
        fetch_tag_from_atom(&client).await
    })
}

async fn fetch_tag_from_api(client: &reqwest::Client) -> Option<String> {
    let resp = client
        .get(release_channel::releases_api_url())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    select_latest_release_tag(&json)
}

async fn fetch_tag_from_atom(client: &reqwest::Client) -> Option<String> {
    let resp = client
        .get(release_channel::releases_atom_url())
        .header("Accept", "application/atom+xml")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    select_latest_release_tag_from_atom(&resp.text().await.ok()?)
}

/// Pull the newest release tag out of the releases Atom feed.
///
/// Entries are newest-first and each carries
/// `<id>tag:github.com,2008:Repository/<repo-id>/<tag></id>`, so the tag is
/// the last path segment of the first entry id. Only that one field is read —
/// this is deliberately a targeted extraction rather than an XML parse, so a
/// feed that grows unrelated markup cannot change the result.
fn select_latest_release_tag_from_atom(feed: &str) -> Option<String> {
    // The feed itself opens with an `<id>` for the repository, which has no
    // `Repository/<id>/<tag>` shape; requiring the entry marker skips it.
    const ENTRY_ID_MARKER: &str = "tag:github.com,2008:Repository/";
    let rest = feed.split_once(ENTRY_ID_MARKER)?.1;
    let id_body = rest.split_once("</id>")?.0;
    let tag = id_body.rsplit_once('/')?.1.trim();
    (!tag.is_empty() && !tag.contains('<')).then(|| tag.to_string())
}

/// Select the newest published release tag from GitHub's releases list.
/// GitHub returns newest releases first. Drafts are not visible to users and
/// must not drive update prompts; prereleases are visible and are intentionally
/// allowed for v0.8.0.
fn select_latest_release_tag(json: &serde_json::Value) -> Option<String> {
    json.as_array()?
        .iter()
        .filter(|release| {
            !release
                .get("draft")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .find_map(|release| release.get("tag_name")?.as_str().map(str::to_string))
}

/// Download the platform installer for `version` on a worker thread
/// and open it when done (macOS mounts the DMG, Windows launches the
/// NSIS setup, Linux reveals the AppImage after `chmod +x`). Any
/// failure — unknown platform, network error, missing asset — falls
/// back to opening the releases page, so the user always ends up
/// with a path to the update. Mirrors the electron-updater flow
/// minus signature verification (no signing pipeline yet; the
/// artifacts are the rust-release.yml installer matrix). Only ever
/// downloads from the fork's own release repository, and not at all
/// while [`release_channel::RELEASES_ENABLED`] is `false`.
pub fn download_and_open_installer(version: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    // One download at a time — a second "Yes" (or re-check) while a
    // worker is still transferring must not spawn a duplicate writer
    // onto the same temp path.
    static IN_FLIGHT: AtomicBool = AtomicBool::new(false);
    if !claim_download(RELEASES_ENABLED, &IN_FLIGHT) {
        return;
    }
    let version = version.to_string();
    std::thread::spawn(move || {
        if !download_and_open_blocking(&version) {
            open_url(&releases_url());
        }
        IN_FLIGHT.store(false, Ordering::SeqCst);
    });
}

/// Whether a download may start now: never while releases are disabled
/// (this build has no signed installer of its own to fetch), and never while
/// another one is in flight. Claims the in-flight flag when it says yes.
fn claim_download(releases_enabled: bool, in_flight: &std::sync::atomic::AtomicBool) -> bool {
    use std::sync::atomic::Ordering;
    releases_enabled
        && in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
}

/// The release-asset filename rust-release.yml publishes for this
/// platform, or `None` on an unsupported os/arch pair.
fn installer_asset_name(version: &str) -> Option<String> {
    asset_name_for(std::env::consts::OS, std::env::consts::ARCH, version)
}

/// Pure name table — split from [`installer_asset_name`] so tests
/// cover every platform from any host.
fn asset_name_for(os: &str, arch: &str, version: &str) -> Option<String> {
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return None,
    };
    Some(match os {
        "macos" => format!("OpenPencil-{version}-{arch}-mac.dmg"),
        "windows" => format!("OpenPencil-{version}-{arch}-win-setup.exe"),
        "linux" => format!("OpenPencil-{version}-{arch}-linux.AppImage"),
        _ => return None,
    })
}

/// Blocking download + open. Only ever runs on the worker thread.
fn download_and_open_blocking(version: &str) -> bool {
    let Some(name) = installer_asset_name(version) else {
        return false;
    };
    let Some(url) = release_channel::configured_installer_url(version, &name) else {
        return false;
    };
    let dest = std::env::temp_dir().join(&name);
    // Shared runtime, not a private one — see `fetch_latest_tag`.
    let downloaded = op_host_services::chat_runtime::block_on_anywhere(async {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            // Installers run tens of MB — allow a long transfer.
            .timeout(std::time::Duration::from_secs(600))
            .user_agent(concat!("openpencil-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        let mut resp = client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        // Stream to disk chunk by chunk — installers run tens of MB
        // and must not be buffered whole in memory.
        let mut file = std::fs::File::create(&dest).ok()?;
        use std::io::Write;
        while let Some(chunk) = resp.chunk().await.ok()? {
            file.write_all(&chunk).ok()?;
        }
        file.flush().ok()?;
        Some(())
    });
    if downloaded.is_none() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
    }
    open_installer_path(&dest)
}

/// Args for `cmd /C start "" "<target>"` with the target double-quoted
/// so cmd doesn't split it at `&` (OAuth / query-string URLs) or at
/// spaces (installer paths). Inside double quotes cmd keeps its
/// metacharacters literal; `"` itself is illegal in URLs and Windows
/// paths but is stripped defensively. `%VAR%` expansion remains
/// possible in pathological inputs — accepted, matches the `open`
/// crate's behaviour.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_start_args(target: &str) -> String {
    format!("/C start \"\" \"{}\"", target.replace('"', "%22"))
}

/// Spawn `cmd /C start "" "<target>"` without flashing a console.
#[cfg(target_os = "windows")]
fn windows_start(target: &str) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new("cmd");
    c.raw_arg(windows_start_args(target));
    c.creation_flags(CREATE_NO_WINDOW);
    c.spawn()
}

/// Open the downloaded installer with the platform launcher.
fn open_installer_path(path: &std::path::Path) -> bool {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let result = windows_start(&path.display().to_string());
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();
    result.is_ok()
}

/// Whether `latest` is a strictly newer version than `current`.
/// Both are dotted numeric strings (`0.8.0`); a pre-release suffix
/// (`-beta.1`) is dropped before the numeric compare so a stable
/// release of the same core version doesn't read as an update.
fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.split('-')
            .next()
            .unwrap_or(v)
            .split('.')
            .map(|n| n.trim().parse::<u64>().unwrap_or(0))
            .collect()
    }
    let l = parts(latest);
    let c = parts(current);
    let len = l.len().max(c.len());
    for i in 0..len {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv != cv {
            return lv > cv;
        }
    }
    false
}

/// Open `url` in the user's default browser. Best-effort: a missing
/// opener tool just logs. Platform openers — `open` (macOS),
/// `xdg-open` (Linux), `cmd /c start` (Windows).
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = windows_start(url);
    if let Err(e) = result {
        eprintln!("openpencil-desktop: open_url({url}) failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped like the live feed: a repository-level `<id>` comes first and
    /// must not be mistaken for a release.
    const ATOM_FEED: &str = concat!(
        "<feed><id>tag:github.com,2008:https://github.com/baroned1707/openpencil/releases</id>",
        "<entry><id>tag:github.com,2008:Repository/1159938129/v0.8.3</id></entry>",
        "<entry><id>tag:github.com,2008:Repository/1159938129/v0.8.2</id></entry></feed>",
    );

    #[test]
    fn atom_fallback_reads_the_newest_release_tag() {
        assert_eq!(
            select_latest_release_tag_from_atom(ATOM_FEED).as_deref(),
            Some("v0.8.3")
        );
    }

    #[test]
    fn atom_fallback_yields_nothing_without_a_release_entry() {
        // A repo with no releases still serves a feed, and its only `<id>` is
        // the repository one. Returning its trailing path segment there would
        // invent a version out of the URL.
        for feed in [
            "<feed><id>tag:github.com,2008:https://github.com/o/r/releases</id></feed>",
            "<feed></feed>",
            "",
            // Truncated mid-entry: no closing tag to bound the id.
            "<feed><entry><id>tag:github.com,2008:Repository/1/v0.8.3",
            // Present but empty tag segment.
            "<feed><entry><id>tag:github.com,2008:Repository/1/</id></entry></feed>",
        ] {
            assert_eq!(
                select_latest_release_tag_from_atom(feed),
                None,
                "{feed:?} must not yield a version"
            );
        }
    }

    #[test]
    fn windows_start_args_quotes_urls_with_query_params() {
        assert_eq!(
            windows_start_args("https://a.example/auth?b=1&c=2"),
            "/C start \"\" \"https://a.example/auth?b=1&c=2\""
        );
        // Double quotes are illegal in URLs/paths — stripped defensively.
        assert_eq!(windows_start_args("x\"y"), "/C start \"\" \"x%22y\"");
    }

    #[test]
    fn is_newer_compares_dotted_numbers() {
        assert!(is_newer("0.9.0", "0.8.0"));
        assert!(is_newer("0.8.1", "0.8.0"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.8.0", "0.8.0"));
        assert!(!is_newer("0.7.9", "0.8.0"));
    }

    #[test]
    fn is_newer_handles_uneven_segment_counts() {
        // A shorter version pads missing trailing segments with 0.
        assert!(is_newer("0.8.0.1", "0.8.0"));
        assert!(!is_newer("0.8", "0.8.0"));
        assert!(is_newer("0.9", "0.8.5"));
    }

    #[test]
    fn is_newer_drops_a_prerelease_suffix() {
        // A stable release equal to the running core version is not
        // "newer"; the `-beta` suffix must not skew the compare.
        assert!(!is_newer("0.8.0-beta.1", "0.8.0"));
        assert!(is_newer("0.9.0-rc.1", "0.8.0"));
    }

    #[test]
    fn releases_api_uses_list_endpoint_so_prereleases_are_visible() {
        assert_eq!(
            release_channel::releases_api_url(),
            "https://api.github.com/repos/baroned1707/openpencil/releases?per_page=20"
        );
    }

    #[test]
    fn release_list_selection_keeps_prerelease_and_skips_drafts() {
        let feed = serde_json::json!([
            {
                "tag_name": "v0.9.0",
                "draft": true,
                "prerelease": false
            },
            {
                "tag_name": "v0.8.0",
                "draft": false,
                "prerelease": true
            },
            {
                "tag_name": "v0.7.5",
                "draft": false,
                "prerelease": false
            }
        ]);

        assert_eq!(select_latest_release_tag(&feed).as_deref(), Some("v0.8.0"));
    }

    #[test]
    fn releases_url_points_at_the_publish_repo() {
        assert_eq!(
            releases_url(),
            "https://github.com/baroned1707/openpencil/releases"
        );
    }

    #[test]
    fn disabled_releases_make_no_request_and_report_unavailable() {
        let mut probe = UpdateProbe::spawn_with(false, || {
            panic!("a disabled probe must never run the release check")
        });

        assert_eq!(probe.poll(), Some(UpdateStatus::Unavailable));
        assert!(!probe.is_pending());
    }

    #[test]
    fn enabled_releases_run_the_check() {
        let mut probe = UpdateProbe::spawn_with(true, || UpdateStatus::UpToDate);

        let status = loop {
            if let Some(status) = probe.poll() {
                break status;
            }
            std::thread::yield_now();
        };

        assert_eq!(status, UpdateStatus::UpToDate);
    }

    #[test]
    fn this_build_probe_resolves_without_a_request() {
        // `spawn` uses the real switch and the real network check: it may
        // only be called here because this build ships with releases off.
        assert!(!RELEASES_ENABLED);
        let mut probe = UpdateProbe::spawn();

        assert_eq!(probe.poll(), Some(UpdateStatus::Unavailable));
    }

    #[test]
    fn auto_check_with_the_preference_on_still_makes_no_request_while_disabled() {
        assert!(!RELEASES_ENABLED);
        let mut probe = UpdateProbe::for_auto_check(true);

        assert_eq!(probe.poll(), Some(UpdateStatus::Unavailable));
    }

    #[test]
    fn a_download_never_starts_while_releases_are_disabled() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let in_flight = AtomicBool::new(false);

        assert!(!claim_download(false, &in_flight));
        assert!(!in_flight.load(Ordering::SeqCst), "nothing was claimed");
        assert!(!claim_download(RELEASES_ENABLED, &in_flight));
    }

    #[test]
    fn an_enabled_download_is_claimed_once_at_a_time() {
        use std::sync::atomic::AtomicBool;
        let in_flight = AtomicBool::new(false);

        assert!(claim_download(true, &in_flight));
        assert!(!claim_download(true, &in_flight));
    }

    #[test]
    fn idle_probe_is_not_pending() {
        let probe = UpdateProbe::idle();

        assert!(!probe.is_pending());
    }

    #[test]
    fn disabled_auto_check_creates_idle_probe() {
        let probe = UpdateProbe::for_auto_check(false);

        assert!(!probe.is_pending());
    }

    #[test]
    fn disconnected_worker_resolves_as_error_instead_of_staying_checking() {
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut probe = UpdateProbe { rx: Some(rx) };

        assert_eq!(probe.poll(), Some(UpdateStatus::Error));
        assert!(!probe.is_pending());
    }
}

#[cfg(test)]
mod installer_asset_tests {
    use super::asset_name_for;

    #[test]
    fn asset_names_cover_the_release_matrix() {
        assert_eq!(
            asset_name_for("macos", "aarch64", "0.9.0").as_deref(),
            Some("OpenPencil-0.9.0-arm64-mac.dmg")
        );
        assert_eq!(
            asset_name_for("windows", "x86_64", "0.9.0").as_deref(),
            Some("OpenPencil-0.9.0-x64-win-setup.exe")
        );
        assert_eq!(
            asset_name_for("linux", "aarch64", "0.9.0").as_deref(),
            Some("OpenPencil-0.9.0-arm64-linux.AppImage")
        );
        assert_eq!(asset_name_for("freebsd", "x86_64", "0.9.0"), None);
        assert_eq!(asset_name_for("macos", "riscv64", "0.9.0"), None);
    }
}
