//! Pixabay / Unsplash / Pexels keys and the Unsplash / Pexels opt-ins: persisted like the
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
    assert!(!settings.pexels_enabled, "Pexels is opt-in");
    assert!(settings.pixabay_api_key.is_blank());
    assert!(settings.unsplash_access_key.is_blank());
    assert!(settings.pexels_api_key.is_blank());
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
        settings.pexels_api_key = " pexels-key ".into();
        settings.pexels_enabled = true;
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
    assert_eq!(raw["stock_providers"]["pexels_api_key"], "pexels-key");
    assert_eq!(raw["stock_providers"]["pexels_enabled"], true);
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
    assert_eq!(settings.pexels_api_key.expose(), "pexels-key");
    assert!(settings.pexels_enabled);
    // The strict loader (the one startup uses) accepts the new fields too.
    let mut strict = EditorState::new();
    load_checked_from_path(&mut strict, &path).expect("the checked loader knows the new fields");
    assert_eq!(
        strict.editor_ui.agent_settings.pexels_api_key.expose(),
        "pexels-key"
    );
    assert!(strict.editor_ui.agent_settings.pexels_enabled);

    // Clearing everything removes the block; an old file without it loads
    // with the defaults (Unsplash off).
    state.editor_ui.agent_settings.pixabay_api_key = "".into();
    state.editor_ui.agent_settings.unsplash_access_key = "".into();
    state.editor_ui.agent_settings.unsplash_enabled = false;
    state.editor_ui.agent_settings.pexels_api_key = "".into();
    state.editor_ui.agent_settings.pexels_enabled = false;
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
    state.editor_ui.agent_settings.pexels_api_key = "PEXELS-SECRET-3".into();
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

/// Invariant: a settings file written before Pexels existed (a
/// `stock_providers` block with only the older fields, or none) loads through
/// BOTH loaders with Pexels off and keyless, and the older fields survive.
#[test]
fn a_settings_file_from_before_pexels_loads_with_pexels_off() {
    let path = scratch_settings_path("pre-pexels");
    // A real file of the previous build: save one, then drop the new fields.
    let mut before = EditorState::new();
    before.editor_ui.agent_settings.pixabay_api_key = "old-px".into();
    before.editor_ui.agent_settings.unsplash_access_key = "old-us".into();
    before.editor_ui.agent_settings.unsplash_enabled = true;
    save_checked_to_path(&before, &path).expect("atomic save");
    let mut raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let block = raw["stock_providers"].as_object_mut().unwrap();
    block.remove("pexels_api_key");
    block.remove("pexels_enabled");
    assert!(!block.contains_key("pexels_api_key") && !block.contains_key("pexels_enabled"));
    std::fs::write(&path, serde_json::to_string(&raw).unwrap()).unwrap();
    for strict in [false, true] {
        let mut state = EditorState::new();
        if strict {
            load_checked_from_path(&mut state, &path).expect("old file is valid");
        } else {
            assert!(load_lenient_from_path(&mut state, &path));
        }
        let settings = &state.editor_ui.agent_settings;
        assert_eq!(settings.pixabay_api_key.expose(), "old-px");
        assert_eq!(settings.unsplash_access_key.expose(), "old-us");
        assert!(settings.unsplash_enabled);
        assert!(settings.pexels_api_key.is_blank(), "strict={strict}");
        assert!(!settings.pexels_enabled, "strict={strict}");
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Invariant: editing the Pexels key or flipping its switch changes the
/// fingerprint, so the autosave notices (a missing field here means the
/// change is never written).
#[test]
fn pexels_changes_trigger_a_save() {
    let base = fingerprint(&EditorState::new());
    let mut keyed = EditorState::new();
    keyed.editor_ui.agent_settings.pexels_api_key = "xk".into();
    assert!(base != fingerprint(&keyed), "a Pexels key edit must save");
    let mut switched = EditorState::new();
    switched.editor_ui.agent_settings.pexels_enabled = true;
    assert!(
        base != fingerprint(&switched),
        "the Pexels switch must save"
    );
}
