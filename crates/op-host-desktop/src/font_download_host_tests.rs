//! The missing-font downloader driven end to end against a fake HTTP source:
//! no test here touches the network. Fonts register into the process-global
//! `jian-skia` registry, so each test owns its own family names and the module
//! is skipped on Windows like `fonts.rs` (DirectWrite + parallel test workers).
#![cfg(not(target_os = "windows"))]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use op_editor_core::font_download::{FontDownloadFailure, FontDownloadRequest, FontDownloadStatus};
use op_editor_core::EditorState;
use op_font_fetch::fake::{catalog_json, metadata_static, metadata_variable, FakeFontSource};
use op_font_fetch::{FetchError, License};
use op_host_native::WidgetHostNative;

use super::FontDownloadHost;
use crate::fonts::FontStore;

const DM_SANS_VF: &[u8] = include_bytes!("../assets/fonts/DMSans-VF.ttf");
const DM_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/DMMono-Regular.ttf");
const DM_MONO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/DMMono-Medium.ttf");
const SPACE_GROTESK_VF: &[u8] = include_bytes!("../assets/fonts/SpaceGrotesk-VF.ttf");

static SEQ: AtomicU64 = AtomicU64::new(0);

struct TempRoot(std::path::PathBuf);
impl TempRoot {
    fn new() -> Self {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("op-font-dl-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    host: WidgetHostNative,
    downloads: FontDownloadHost,
    source: Arc<FakeFontSource>,
    root: TempRoot,
}

fn document(runs: &[(&str, u32)]) -> EditorState {
    let children: Vec<_> = runs
        .iter()
        .enumerate()
        .map(|(index, (family, weight))| {
            serde_json::json!({
                "type": "text", "id": format!("t{index}"), "content": "Hamburgefons",
                "fontFamily": family, "fontWeight": weight,
            })
        })
        .collect();
    let doc = serde_json::from_value(serde_json::json!({"version": "1.0.0", "children": children}))
        .expect("document");
    EditorState::from_document(doc)
}

fn rig(runs: &[(&str, u32)], auto: bool) -> Rig {
    for family in ["DM Sans", "DM Mono", "Space Grotesk"] {
        if runs.iter().any(|(name, _)| *name == family) {
            jian_skia::remove_imported_font(family);
        }
    }
    let source = Arc::new(FakeFontSource::new());
    source.serve_catalog(&[
        ("dm-sans", "DM Sans", "OFL-1.1"),
        ("dm-mono", "DM Mono", "OFL-1.1"),
        ("space-grotesk", "Space Grotesk", "OFL-1.1"),
        ("cc-zero-face", "Cc Zero Face", "CC0-1.0"),
    ]);
    source.serve_family(
        "dm-sans",
        License::Ofl,
        &metadata_variable("DM Sans", "OFL", &[("DMSans[opsz,wght].ttf", "normal")]),
        &[("DMSans[opsz,wght].ttf", DM_SANS_VF)],
    );
    source.serve_family(
        "space-grotesk",
        License::Ofl,
        &metadata_variable(
            "Space Grotesk",
            "OFL",
            &[("SpaceGrotesk[wght].ttf", "normal")],
        ),
        &[("SpaceGrotesk[wght].ttf", SPACE_GROTESK_VF)],
    );
    source.serve_family(
        "dm-mono",
        License::Ofl,
        &metadata_static(
            "DM Mono",
            "OFL",
            &[
                ("DMMono-Regular.ttf", "normal", 400),
                ("DMMono-Medium.ttf", "normal", 500),
            ],
        ),
        &[
            ("DMMono-Regular.ttf", DM_MONO_REGULAR),
            ("DMMono-Medium.ttf", DM_MONO_MEDIUM),
        ],
    );
    let root = TempRoot::new();
    let downloads = FontDownloadHost::with_parts(source.clone(), FontStore::at(&root.0));
    let mut host = WidgetHostNative::new();
    *host.editor_state_mut() = document(runs);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.system_fonts_loaded = true;
    ui.font_downloads.supported = true;
    ui.agent_settings.auto_download_missing_fonts = auto;
    host.refresh_imported_fonts();
    host.refresh_missing_fonts_prompt();
    Rig {
        host,
        downloads,
        source,
        root,
    }
}

impl Rig {
    /// Run frames until the downloader is idle (or fail after 15 s).
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            self.downloads.drain(&mut self.host);
            if !self.downloads.is_busy() {
                // One more frame lands anything that finished in between.
                self.downloads.drain(&mut self.host);
                return;
            }
            assert!(Instant::now() < deadline, "downloader never went idle");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn click(&mut self, request: FontDownloadRequest) {
        self.host
            .editor_state_mut()
            .editor_ui
            .font_downloads
            .pending
            .push(request);
    }

    fn missing(&self) -> Vec<String> {
        self.host
            .editor_state()
            .editor_ui
            .missing_fonts_prompt
            .as_ref()
            .map(|prompt| prompt.entries.iter().map(|e| e.family.clone()).collect())
            .unwrap_or_default()
    }

    fn status(&self, family: &str) -> Option<FontDownloadStatus> {
        self.host
            .editor_state()
            .editor_ui
            .font_downloads
            .status(family)
    }

    fn font_requests(&self) -> usize {
        self.source
            .requests()
            .iter()
            .filter(|url| url.ends_with(".ttf"))
            .count()
    }
}

fn face_count(family: &str) -> usize {
    jian_skia::list_families()
        .iter()
        .find(|meta| meta.family == family)
        .map_or(0, |meta| meta.face_count)
}

#[test]
fn download_action_resolves_the_row_and_bumps_font_generation() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("DM Sans", 700)], false);
    assert_eq!(rig.missing(), ["DM Sans"]);

    // A resolver built BEFORE the download must pick the family up afterwards.
    let before_generation = jian_skia::font_generation();
    let resolver = jian_skia::font_resolve::FontResolver::new(skia_safe::FontMgr::new());
    let before = resolver
        .typeface_for_char(Some("DM Sans"), 'H', 700, false)
        .expect("fallback face");
    assert_ne!(before.typeface.family_name(), "DM Sans");

    rig.click(FontDownloadRequest::Family("DM Sans".into()));
    rig.settle();

    assert!(jian_skia::font_generation() > before_generation);
    assert_eq!(
        face_count("DM Sans"),
        1,
        "one variable file, registered once"
    );
    assert!(
        rig.missing().is_empty(),
        "the row resolved: {:?}",
        rig.missing()
    );
    assert!(rig
        .host
        .editor_state()
        .editor_ui
        .missing_fonts_prompt
        .is_none());
    let after = resolver
        .typeface_for_char(Some("DM Sans"), 'H', 700, false)
        .expect("downloaded face");
    assert_eq!(after.typeface.family_name(), "DM Sans");
    jian_skia::remove_imported_font("DM Sans");
}

#[test]
fn setting_off_fetches_nothing_automatically() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], false);
    for _ in 0..20 {
        rig.downloads.drain(&mut rig.host);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(rig.source.request_count(), 0, "{:?}", rig.source.requests());
    assert_eq!(rig.missing(), ["Space Grotesk"]);
    assert!(!rig.downloads.is_busy());

    // Only the user's click starts it.
    rig.click(FontDownloadRequest::All);
    rig.settle();
    assert!(rig.source.request_count() > 0);
    assert!(rig.missing().is_empty());
    jian_skia::remove_imported_font("Space Grotesk");
}

#[test]
fn auto_download_starts_once_per_document_open() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], true);
    rig.settle();
    assert!(rig.missing().is_empty(), "auto download resolved the row");
    let after_first = rig.source.request_count();
    assert_eq!(rig.font_requests(), 1, "one variable file");

    // The same document is opened again and detection runs again (the font was
    // removed meanwhile): the session already handled this family, so nothing
    // is fetched a second time.
    jian_skia::remove_imported_font("Space Grotesk");
    rig.host.refresh_imported_fonts();
    *rig.host.editor_state_mut() = {
        let mut state = document(&[("Space Grotesk", 400)]);
        let old = &rig.host.editor_state().editor_ui;
        state.editor_ui.system_fonts_loaded = true;
        state.editor_ui.font_downloads = old.font_downloads.clone();
        state.editor_ui.agent_settings.auto_download_missing_fonts = true;
        state
    };
    rig.host.refresh_imported_fonts();
    rig.host.refresh_missing_fonts_prompt();
    assert_eq!(rig.missing(), ["Space Grotesk"]);
    for _ in 0..10 {
        rig.downloads.drain(&mut rig.host);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        rig.source.request_count(),
        after_first,
        "no refetch on re-open"
    );
    assert!(!rig.downloads.is_busy());
    jian_skia::remove_imported_font("Space Grotesk");
}

#[test]
fn unknown_and_disallowed_rows_stay_missing_with_a_reason() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Helvetica Neue", 400), ("Cc Zero Face", 400)], true);
    rig.settle();
    assert_eq!(rig.missing().len(), 2);
    assert_eq!(
        rig.status("Helvetica Neue"),
        Some(FontDownloadStatus::Failed(FontDownloadFailure::UnknownFont))
    );
    assert_eq!(
        rig.status("Cc Zero Face"),
        Some(FontDownloadStatus::Failed(
            FontDownloadFailure::LicenseNotAllowed
        ))
    );
    assert_eq!(rig.font_requests(), 0);
    // Neither offers a Retry: the answer will not change.
    let ui = &rig.host.editor_state().editor_ui;
    for entry in &ui.missing_fonts_prompt.as_ref().unwrap().entries {
        assert_eq!(
            ui.font_downloads.row_action(entry),
            None,
            "{}",
            entry.family
        );
    }
}

#[test]
fn offline_marks_rows_and_retry_succeeds_once_back_online() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], true);
    rig.source.set_offline(true);
    rig.settle();
    assert_eq!(
        rig.status("Space Grotesk"),
        Some(FontDownloadStatus::Failed(FontDownloadFailure::Offline))
    );
    assert_eq!(rig.missing(), ["Space Grotesk"], "the notice stays");
    let attempted = rig.source.request_count();
    for _ in 0..10 {
        rig.downloads.drain(&mut rig.host);
    }
    assert_eq!(rig.source.request_count(), attempted, "no automatic retry");

    rig.source.set_offline(false);
    rig.click(FontDownloadRequest::Family("Space Grotesk".into()));
    rig.settle();
    assert!(rig.missing().is_empty());
    jian_skia::remove_imported_font("Space Grotesk");
}

#[test]
fn static_family_registers_each_requested_weight() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("DM Mono", 400), ("DM Mono", 500)], true);
    rig.settle();
    assert!(rig.missing().is_empty(), "{:?}", rig.missing());
    assert_eq!(rig.font_requests(), 2, "Regular and Medium, nothing else");
    assert_eq!(face_count("DM Mono"), 2);

    let resolver = jian_skia::font_resolve::FontResolver::new(skia_safe::FontMgr::new());
    for (requested, expected) in [(400, 400), (500, 500)] {
        let face = resolver
            .typeface_for_char(Some("DM Mono"), 'H', requested, false)
            .expect("downloaded face");
        assert_eq!(face.typeface.family_name(), "DM Mono");
        assert_eq!(*face.typeface.font_style().weight(), expected);
    }
    jian_skia::remove_imported_font("DM Mono");
}

#[test]
fn download_that_does_not_parse_is_not_registered() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], false);
    // Right magic bytes, so the fetch layer accepts it, but skia cannot read it.
    rig.source.route_bytes(
        "https://raw.githubusercontent.com/google/fonts/main/ofl/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf",
        op_font_fetch::fake::TTF_STUB,
    );
    rig.click(FontDownloadRequest::Family("Space Grotesk".into()));
    rig.settle();
    assert_eq!(face_count("Space Grotesk"), 0, "nothing was registered");
    assert_eq!(rig.missing(), ["Space Grotesk"]);
    assert_eq!(
        rig.status("Space Grotesk"),
        Some(FontDownloadStatus::Failed(FontDownloadFailure::Invalid))
    );
    assert!(
        !rig.root.0.join("fonts").join("licenses").exists(),
        "no licence or font file is written for a rejected download"
    );
}

#[test]
fn a_file_of_another_family_is_not_registered_under_the_requested_name() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], false);
    // The directory serves DM Sans bytes where Space Grotesk was promised.
    rig.source.route_bytes(
        "https://raw.githubusercontent.com/google/fonts/main/ofl/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf",
        DM_SANS_VF,
    );
    jian_skia::remove_imported_font("DM Sans");
    rig.click(FontDownloadRequest::Family("Space Grotesk".into()));
    rig.settle();
    assert_eq!(face_count("DM Sans"), 0);
    assert_eq!(face_count("Space Grotesk"), 0);
    assert_eq!(rig.missing(), ["Space Grotesk"]);
}

#[test]
fn licence_file_is_stored_beside_the_font() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], true);
    rig.settle();
    let store = FontStore::at(&rig.root.0);
    let dir = store.licenses_dir().join("space-grotesk");
    assert_eq!(std::fs::read(dir.join("OFL.txt")).unwrap(), b"licence text");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("source.json")).unwrap()).unwrap();
    assert_eq!(record["family"], "Space Grotesk");
    assert_eq!(record["catalogId"], "space-grotesk");
    assert_eq!(record["license"], "OFL-1.1");
    assert!(record["files"][0]["url"]
        .as_str()
        .unwrap()
        .starts_with("https://raw.githubusercontent.com/google/fonts/main/ofl/spacegrotesk/"));
    assert!(record["downloadedAtUnix"].as_u64().unwrap() > 0);
    // The face itself is in the shared font store and survives a restart.
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(rig.root.0.join("fonts/index.json")).unwrap())
            .unwrap();
    assert_eq!(index["fonts"][0]["family"], "Space Grotesk");
    jian_skia::remove_imported_font("Space Grotesk");
    FontStore::at(&rig.root.0).rescan_and_register();
    assert_eq!(
        face_count("Space Grotesk"),
        1,
        "restored by the startup rescan"
    );
    jian_skia::remove_imported_font("Space Grotesk");
}

#[test]
fn a_family_the_catalog_lacks_costs_one_catalog_request_and_no_font_request() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("../../etc/passwd", 400)], true);
    rig.source.route(
        "https://api.fontsource.org/v1/fonts",
        Err(FetchError::Status(500)),
    );
    rig.settle();
    assert_eq!(rig.font_requests(), 0);
    for url in rig.source.requests() {
        assert!(url.starts_with("https://api.fontsource.org/"), "{url}");
    }
    let _ = catalog_json(&[]);
}

#[test]
fn a_removed_font_brings_the_row_back_with_retry_not_a_dead_label() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut rig = rig(&[("Space Grotesk", 400)], true);
    rig.settle();
    assert!(rig.missing().is_empty());
    assert_eq!(
        rig.status("Space Grotesk"),
        Some(FontDownloadStatus::Downloaded)
    );

    // The user deletes the font in Settings > Fonts; the row returns.
    jian_skia::remove_imported_font("Space Grotesk");
    rig.host.refresh_imported_fonts();
    rig.host.refresh_missing_fonts_prompt();
    assert_eq!(rig.missing(), ["Space Grotesk"]);
    let requests = rig.source.request_count();

    rig.settle();
    assert_eq!(
        rig.status("Space Grotesk"),
        Some(FontDownloadStatus::Failed(FontDownloadFailure::Other))
    );
    let ui = &rig.host.editor_state().editor_ui;
    let row = &ui.missing_fonts_prompt.as_ref().unwrap().entries[0];
    assert!(
        ui.font_downloads.row_action(row).is_some(),
        "Retry is offered"
    );
    assert_eq!(
        rig.source.request_count(),
        requests,
        "a removal is not re-fetched on its own"
    );

    rig.click(FontDownloadRequest::Family("Space Grotesk".into()));
    rig.settle();
    assert!(rig.missing().is_empty());
    jian_skia::remove_imported_font("Space Grotesk");
}
