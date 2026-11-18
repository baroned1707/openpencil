//! Pixabay / Unsplash keys and the Unsplash opt-in: persisted like the
//! Openverse fields, off by default, never printed by `Debug`.

use super::super::*;

fn scratch_settings_path(case: &str) -> std::path::PathBuf {
    let sequence = SETTINGS_TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "openpencil-settings-stock-{}-{sequence}-{case}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.join("settings.json")
}

#[test]
fn unsplash_toggle_defaults_off() {
    let state = EditorState::new();
    let settings = &state.editor_ui.agent_settings;
    assert!(!settings.unsplash_enabled, "Unsplash is opt-in");
    assert!(settings.pixabay_api_key.is_blank());
    assert!(settings.unsplash_access_key.is_blank());
    // A user who never configured stock providers writes nothing for them,
    // so the settings file keeps its previous shape.
    assert!(to_payload(&state).stock_providers.is_none());
    let json = serde_json::to_value(to_payload(&state)).unwrap();
    assert!(json.get("stock_providers").is_none());
}

#[test]
fn settings_round_trip_new_fields() {
    let mut state = EditorState::new();
    {
        let settings = &mut state.editor_ui.agent_settings;
        settings.pixabay_api_key = " pixabay-key ".into();
        settings.unsplash_access_key = "unsplash-key".into();
        settings.unsplash_enabled = true;
    }
    let before = fingerprint(&EditorState::new());
    assert!(
        before != fingerprint(&state),
        "a key edit must trigger a save"
    );

    let path = scratch_settings_path("round-trip");
    save_checked_to_path(&state, &path).expect("atomic save");
    let written = std::fs::read_to_string(&path).unwrap();
    let raw: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(raw["stock_providers"]["pixabay_api_key"], "pixabay-key");
    assert_eq!(
        raw["stock_providers"]["unsplash_access_key"],
        "unsplash-key"
    );
    assert_eq!(raw["stock_providers"]["unsplash_enabled"], true);
    assert!(settings_io_checked::validate_payload_fields(&raw).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the key file stays owner-only");
    }

    let mut restored = EditorState::new();
    assert!(load_lenient_from_path(&mut restored, &path));
    let settings = &restored.editor_ui.agent_settings;
    assert_eq!(settings.pixabay_api_key.expose(), "pixabay-key");
    assert_eq!(settings.unsplash_access_key.expose(), "unsplash-key");
    assert!(settings.unsplash_enabled);

    // Clearing everything removes the block; an old file without it loads
    // with the defaults (Unsplash off).
    state.editor_ui.agent_settings.pixabay_api_key = "".into();
    state.editor_ui.agent_settings.unsplash_access_key = "".into();
    state.editor_ui.agent_settings.unsplash_enabled = false;
    save_checked_to_path(&state, &path).expect("atomic save");
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(raw.get("stock_providers").is_none());
    let mut legacy = EditorState::new();
    legacy.editor_ui.agent_settings.unsplash_enabled = true;
    assert!(load_lenient_from_path(&mut legacy, &path));
    assert!(!legacy.editor_ui.agent_settings.unsplash_enabled);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn keys_are_redacted_in_debug_output() {
    let mut state = EditorState::new();
    state.editor_ui.agent_settings.pixabay_api_key = "PIXABAY-SECRET-1".into();
    state.editor_ui.agent_settings.unsplash_access_key = "UNSPLASH-SECRET-2".into();
    let payload = to_payload(&state);
    // The payload, the whole settings struct and the fingerprint all derive
    // Debug; none of them may print a key.
    for dump in [
        format!("{payload:?}"),
        format!("{:#?}", payload),
        format!("{:?}", state.editor_ui.agent_settings),
        format!("{:?}", fingerprint(&state)),
    ] {
        assert!(!dump.contains("SECRET"), "{dump}");
    }
}
