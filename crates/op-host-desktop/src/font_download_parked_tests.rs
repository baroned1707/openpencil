//! Missing fonts of a parked document are downloaded without switching to it,
//! through the one shared fetcher (a fake source: no test touches the network).
//! Fonts register into the process-global `jian-skia` registry, so each test
//! owns its family names and the module is skipped on Windows like
//! `font_download_host_tests.rs`.
#![cfg(not(target_os = "windows"))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use op_editor_core::EditorState;
use op_font_fetch::fake::{metadata_static, metadata_variable, FakeFontSource};
use op_font_fetch::License;
use op_host_services::doc_io::save_to_path;

use super::*;
use crate::fonts::FontStore;

const DM_SANS_VF: &[u8] = include_bytes!("../assets/fonts/DMSans-VF.ttf");
const DM_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/DMMono-Regular.ttf");
const DM_MONO_MEDIUM: &[u8] = include_bytes!("../assets/fonts/DMMono-Medium.ttf");

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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-pfont-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn face_count(family: &str) -> usize {
    jian_skia::list_families()
        .iter()
        .find(|meta| meta.family == family)
        .map_or(0, |meta| meta.face_count)
}

struct World {
    app: DesktopApp,
    source: Arc<FakeFontSource>,
    dir: std::path::PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl World {
    /// One document per entry of `docs` (the last is on screen); every parked
    /// one is edited once so its content counts as changed.
    fn new(tag: &str, docs: &[&[(&str, u32)]]) -> Self {
        for family in ["DM Sans", "DM Mono"] {
            jian_skia::remove_imported_font(family);
        }
        let source = Arc::new(FakeFontSource::new());
        source.serve_catalog(&[
            ("dm-sans", "DM Sans", "OFL-1.1"),
            ("dm-mono", "DM Mono", "OFL-1.1"),
        ]);
        source.serve_family(
            "dm-sans",
            License::Ofl,
            &metadata_variable("DM Sans", "OFL", &[("DMSans[opsz,wght].ttf", "normal")]),
            &[("DMSans[opsz,wght].ttf", DM_SANS_VF)],
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
        let dir = temp_dir(tag);
        let mut app = DesktopApp::new(None);
        app.font_downloads =
            FontDownloadHost::with_parts(source.clone(), FontStore::at(dir.join("fonts")));
        let ui = &mut app.host.editor_state_mut().editor_ui;
        ui.system_fonts_loaded = true;
        ui.font_downloads.supported = true;
        ui.agent_settings.auto_download_missing_fonts = true;
        for (index, runs) in docs.iter().enumerate() {
            let path = dir.join(format!("doc{index}.op"));
            save_to_path(&document(runs), &path).expect("fixture saves");
            assert!(app.open_document_tab(path));
        }
        let on_screen = app.documents.active_index();
        for index in 0..docs.len() - 1 {
            assert!(app.activate_document_slot(index));
            app.host.editor_state_mut().mark_document_changed();
        }
        assert!(app.activate_document_slot(on_screen));
        Self { app, source, dir }
    }

    /// Frames of the downloader until it is idle (or fail after 15 s). `now`
    /// advances past the scan gap each frame.
    fn settle(&mut self) {
        let mut now = Instant::now();
        let deadline = now + Duration::from_secs(15);
        loop {
            self.app.font_downloads.drain(&mut self.app.host);
            self.app.pump_parked_font_downloads(now);
            if !self.app.font_downloads.is_busy() {
                self.app.font_downloads.drain(&mut self.app.host);
                // Idle: but a frame may still be about to start the batch.
                self.app
                    .pump_parked_font_downloads(now + Duration::from_secs(1));
                if !self.app.font_downloads.is_busy() {
                    return;
                }
            }
            assert!(Instant::now() < deadline, "downloader never went idle");
            now += Duration::from_millis(600);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn font_requests(&self) -> usize {
        self.source
            .requests()
            .iter()
            .filter(|url| url.ends_with(".ttf"))
            .count()
    }
}

#[test]
fn a_parked_documents_missing_font_downloads_without_switching() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut world = World::new("single", &[&[("DM Sans", 400)], &[("Arial", 400)]]);
    let on_screen = world.app.documents.active_id();
    assert_eq!(face_count("DM Sans"), 0);

    world.settle();

    assert!(face_count("DM Sans") >= 1, "the family is registered");
    assert_eq!(world.font_requests(), 1);
    assert_eq!(world.app.documents.active_id(), on_screen, "no tab switch");
}

#[test]
fn two_documents_wanting_one_family_download_it_once_with_every_face() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut world = World::new(
        "merge",
        &[&[("DM Mono", 400)], &[("DM Mono", 500)], &[("Arial", 400)]],
    );

    world.settle();

    assert_eq!(face_count("DM Mono"), 2, "regular and medium");
    assert_eq!(
        world.font_requests(),
        2,
        "each face file is fetched once, not once per document"
    );
}

#[test]
fn nothing_is_downloaded_when_the_setting_is_off() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let mut world = World::new("off", &[&[("DM Sans", 400)], &[("Arial", 400)]]);
    world
        .app
        .host
        .editor_state_mut()
        .editor_ui
        .agent_settings
        .auto_download_missing_fonts = false;

    assert!(!world.app.pump_parked_font_downloads(Instant::now()));
    assert!(!world.app.font_downloads.is_busy());
    assert_eq!(world.font_requests(), 0);
}

#[test]
fn a_parked_document_is_rechecked_only_when_changed_and_past_the_gap() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    let world = World::new("unedited", &[&[("DM Sans", 400)], &[("Arial", 400)]]);
    let candidates = world.app.documents.parked_font_candidates();
    assert_eq!(candidates.len(), 1, "the edited parked document");
    let mut scans = ParkedFontScans::default();
    let now = Instant::now();
    let (_, id, revision) = candidates[0];
    assert!(scans.due(id, revision, now));
    scans.record(id, revision, now);
    assert!(
        !scans.due(id, revision, now + Duration::from_secs(5)),
        "same revision"
    );
    assert!(
        !scans.due(id, revision + 1, now + Duration::from_millis(100)),
        "inside the gap"
    );
    assert!(scans.due(id, revision + 1, now + Duration::from_secs(1)));
}
