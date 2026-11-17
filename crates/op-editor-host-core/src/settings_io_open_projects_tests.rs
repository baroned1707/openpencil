//! The open-project list in the settings file: old files still load, the list
//! is deduplicated and bounded, and a change trips the fingerprint.

use super::*;
use crate::settings_payload::MAX_OPEN_PROJECTS;

#[test]
fn open_projects_round_trip_and_old_files_without_the_field_still_load() {
    let mut source = EditorState::new();
    source.editor_ui.open_projects = vec!["/work/shop".to_owned(), "/work/blog".to_owned()];
    let json = serde_json::to_string(&to_payload(&source)).unwrap();
    assert!(json.contains("\"open_projects\""));
    let mut restored = EditorState::new();
    apply_payload(&mut restored, serde_json::from_str(&json).unwrap());
    assert_eq!(
        restored.editor_ui.open_projects,
        source.editor_ui.open_projects
    );

    let legacy = r#"{"version":1,"theme":"dark"}"#;
    let parsed: SettingsPayload = serde_json::from_str(legacy).unwrap();
    assert_eq!(parsed.open_projects, None);
    let mut older = EditorState::new();
    older.editor_ui.open_projects = vec!["/kept".to_owned()];
    apply_payload(&mut older, parsed);
    assert_eq!(older.editor_ui.open_projects, vec!["/kept".to_owned()]);
}

#[test]
fn a_hand_edited_project_list_is_deduplicated_blank_free_and_capped() {
    let mut entries: Vec<String> = vec![
        "/a".into(),
        "".into(),
        "  ".into(),
        "/b".into(),
        "/a".into(),
    ];
    entries.extend((0..MAX_OPEN_PROJECTS + 20).map(|n| format!("/p{n}")));
    let kept = open_projects_from_payload(entries);
    assert_eq!(&kept[..2], ["/a".to_owned(), "/b".to_owned()]);
    assert_eq!(kept.len(), MAX_OPEN_PROJECTS);
    assert_eq!(kept.iter().filter(|path| path.as_str() == "/a").count(), 1);
}

#[test]
fn changing_the_open_projects_trips_the_fingerprint() {
    let mut state = EditorState::new();
    let before = fingerprint(&state);
    state.editor_ui.open_projects = vec!["/a".to_owned()];
    assert_ne!(before, fingerprint(&state));
}

#[test]
fn the_strict_loader_accepts_a_file_that_carries_open_projects() {
    let mut state = EditorState::new();
    state.editor_ui.open_projects = vec!["/a".to_owned()];
    let value = serde_json::to_value(to_payload(&state)).unwrap();
    assert!(
        settings_io_checked::validate_payload_fields(&value).is_ok(),
        "`open_projects` must be a known root field"
    );
}
