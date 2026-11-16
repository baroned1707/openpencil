//! The open-document list in the settings file, and the guarantee that
//! switching documents never changes what the file holds.

use super::*;
use op_editor_core::{DocumentScope, SessionDocument};

fn session(path: &str, active: bool) -> SessionDocument {
    SessionDocument {
        path: path.to_owned(),
        active,
    }
}

#[test]
fn settings_payload_round_trips_old_files_without_open_documents() {
    let mut src = EditorState::new();
    src.editor_ui.session_documents = vec![session("/tmp/a.op", false), session("/tmp/b.op", true)];
    let json = serde_json::to_string(&to_payload(&src)).unwrap();
    assert!(json.contains("\"open_documents\""));
    let mut restored = EditorState::new();
    apply_payload(&mut restored, serde_json::from_str(&json).unwrap());
    assert_eq!(
        restored.editor_ui.session_documents,
        src.editor_ui.session_documents
    );

    // A file written before the field existed loads with nothing to restore.
    let legacy = r#"{"version":1,"theme":"dark"}"#;
    let mut older = EditorState::new();
    older.editor_ui.session_documents = vec![session("/stale.op", true)];
    apply_payload(&mut older, serde_json::from_str(legacy).unwrap());
    assert_eq!(
        older.editor_ui.session_documents,
        vec![session("/stale.op", true)],
        "an absent field leaves the in-memory list alone"
    );
    assert_eq!(
        serde_json::from_str::<SettingsPayload>(legacy)
            .unwrap()
            .open_documents,
        None
    );
}

#[test]
fn a_hand_edited_list_keeps_one_active_entry_and_no_empty_paths() {
    let payload: SettingsPayload = serde_json::from_str(
        r#"{"version":1,"open_documents":[
            {"path":"/a.op","active":true},
            {"path":"","active":true},
            {"path":"/b.op","active":true},
            {"path":"/c.op"}]}"#,
    )
    .unwrap();
    let mut state = EditorState::new();
    apply_payload(&mut state, payload);
    assert_eq!(
        state.editor_ui.session_documents,
        vec![
            session("/a.op", true),
            session("/b.op", false),
            session("/c.op", false)
        ]
    );
}

#[test]
fn changing_the_open_documents_trips_the_fingerprint() {
    let mut state = EditorState::new();
    let before = fingerprint(&state);
    state.editor_ui.session_documents = vec![session("/a.op", true)];
    assert_ne!(before, fingerprint(&state));
}

#[test]
fn swap_roundtrip_preserves_settings_payload() {
    let mut live = EditorState::new();
    live.editor_ui.locale = Locale::Ja;
    live.editor_ui.session_documents = vec![session("/a.op", true)];
    live.editor_ui.agent_settings.mcp_server.port = 4321;
    let before = serde_json::to_string(&to_payload(&live)).unwrap();
    let fingerprint_before = fingerprint(&live);

    let mut scope = DocumentScope::from_state(EditorState::new());
    live.exchange_document_scope(&mut scope);
    assert_eq!(
        serde_json::to_string(&to_payload(&live)).unwrap(),
        before,
        "the document on screen changed, the app settings did not"
    );
    live.exchange_document_scope(&mut scope);

    assert_eq!(serde_json::to_string(&to_payload(&live)).unwrap(), before);
    assert_eq!(fingerprint(&live), fingerprint_before);
}
