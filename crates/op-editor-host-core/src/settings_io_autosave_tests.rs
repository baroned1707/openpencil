//! `autosave_on_agent_run_end`: defaults on, survives a save/load round trip
//! through the real settings file (atomic save, validated field list), and a
//! file that never had the field keeps it on.

use super::super::*;

fn scratch_settings_path(case: &str) -> std::path::PathBuf {
    let sequence = SETTINGS_TEMP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "openpencil-settings-autosave-{}-{sequence}-{case}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.join("settings.json")
}

/// Invariant: the setting is ON for a fresh state, an explicit OFF round-trips
/// through the settings file, a flip is noticed by the change detector (so it
/// is saved), and a payload without the field (a file written before it
/// existed) leaves it ON.
#[test]
fn setting_defaults_true_and_round_trips() {
    let mut state = EditorState::new();
    assert!(state.editor_ui.agent_settings.autosave_on_agent_run_end);
    assert_eq!(to_payload(&state).autosave_on_agent_run_end, Some(true));

    let before = fingerprint(&state);
    state.editor_ui.agent_settings.autosave_on_agent_run_end = false;
    assert!(before != fingerprint(&state), "a flip must trigger a save");

    let path = scratch_settings_path("round-trip");
    save_checked_to_path(&state, &path).expect("atomic save");
    let written = std::fs::read_to_string(&path).unwrap();
    assert!(
        written.contains("\"autosave_on_agent_run_end\": false")
            || written.contains("\"autosave_on_agent_run_end\":false"),
        "{written}"
    );
    // The field list validation of a checked load accepts it.
    let raw: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert!(settings_io_checked::validate_payload_fields(&raw).is_ok());

    let mut restored = EditorState::new();
    assert!(load_lenient_from_path(&mut restored, &path));
    assert!(!restored.editor_ui.agent_settings.autosave_on_agent_run_end);

    // A settings file written before the field existed keeps the default.
    let mut legacy = EditorState::new();
    let mut payload = to_payload(&legacy);
    payload.autosave_on_agent_run_end = None;
    apply_payload(&mut legacy, payload);
    assert!(legacy.editor_ui.agent_settings.autosave_on_agent_run_end);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
