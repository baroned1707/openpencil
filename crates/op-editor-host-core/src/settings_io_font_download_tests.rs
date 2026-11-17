//! `auto_download_missing_fonts`: defaults on, survives a save/load round trip
//! through the real settings file, is watched for changes, and a file that
//! never had the field keeps it on.

use super::super::*;

fn scratch_settings_path(case: &str) -> std::path::PathBuf {
    let sequence = SETTINGS_TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "openpencil-settings-fontdl-{}-{sequence}-{case}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.join("settings.json")
}

/// Invariant: ON for a fresh state, an explicit OFF round-trips (and back ON),
/// a flip triggers a save, the checked loader accepts the field, and a payload
/// without it (a file from before this setting) leaves it ON.
#[test]
fn settings_toggle_round_trips_and_defaults_on() {
    let mut state = EditorState::new();
    assert!(state.editor_ui.agent_settings.auto_download_missing_fonts);
    assert_eq!(to_payload(&state).auto_download_missing_fonts, Some(true));

    let before = fingerprint(&state);
    state.editor_ui.agent_settings.auto_download_missing_fonts = false;
    assert!(before != fingerprint(&state), "a flip must trigger a save");

    let path = scratch_settings_path("round-trip");
    for wanted in [false, true, false] {
        state.editor_ui.agent_settings.auto_download_missing_fonts = wanted;
        save_checked_to_path(&state, &path).expect("atomic save");
        let written = std::fs::read_to_string(&path).unwrap();
        let raw: serde_json::Value = serde_json::from_str(&written).unwrap();
        assert_eq!(raw["auto_download_missing_fonts"], wanted);
        assert!(settings_io_checked::validate_payload_fields(&raw).is_ok());
        let mut restored = EditorState::new();
        assert!(load_lenient_from_path(&mut restored, &path));
        assert_eq!(
            restored
                .editor_ui
                .agent_settings
                .auto_download_missing_fonts,
            wanted,
            "restored after saving {wanted}"
        );
    }

    let mut legacy = EditorState::new();
    let mut payload = to_payload(&legacy);
    payload.auto_download_missing_fonts = None;
    apply_payload(&mut legacy, payload);
    assert!(legacy.editor_ui.agent_settings.auto_download_missing_fonts);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
