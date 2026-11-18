use super::{load_editor_state_from_source, preserve_app_preferences, save_to_path};
use op_editor_core::{EditorState, HomeFamily, Locale, TaskDraft, UsageView};

#[test]
fn every_home_work_kind_survives_save_and_open_without_publishing_the_brief() {
    for family in HomeFamily::ALL {
        let doc = jian_ops_schema::load_str(r#"{"version":"1.0.0","children":[{"type":"frame","id":"work","width":1080,"height":1440,"children":[]}]}"#).unwrap().value;
        let mut state = EditorState::from_document(doc);
        state.editor_ui.open_workspace_for_generation(
            family,
            "PRIVATE BRIEF MUST NOT BE SAVED",
            TaskDraft::default(),
            0,
            1,
            None,
        );
        let path = std::env::temp_dir().join(format!(
            "openpencil-work-kind-{}-{}.op",
            std::process::id(),
            family.id()
        ));
        save_to_path(&state, &path).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!source.contains("PRIVATE BRIEF"));
        let json: serde_json::Value = serde_json::from_str(&source).unwrap();
        assert_eq!(json["editorMeta"]["workFamily"], family.id());
        assert!(json["editorMeta"].get("shareRecipe").is_none());
        let mut reopened = load_editor_state_from_source(&source, Locale::ZhCn).unwrap();
        op_editor_core::work_identity::open_for_reading(&mut reopened, 2);
        assert_eq!(reopened.editor_ui.workspace.family, family);
        assert!(reopened.editor_ui.workspace.family_known);
        assert!(reopened.editor_ui.workspace.brief.is_empty());
        assert_eq!(reopened.doc, state.doc);
    }
}

#[test]
fn legacy_unknown_and_malformed_work_tags_remain_viewable_and_unclassified() {
    for tag in ["null", "7", "\"future-kind\"", "{\"family\":\"poster\"}"] {
        let source =
            format!(r#"{{"version":"1.0.0","children":[],"editorMeta":{{"workFamily":{tag}}}}}"#);
        let mut state = load_editor_state_from_source(&source, Locale::ZhCn).unwrap();
        op_editor_core::work_identity::open_for_reading(&mut state, 1);
        assert!(state.editor_ui.home.work_family.is_none());
        assert!(!state.editor_ui.workspace.family_known);
    }
}

// The chat usage meter is in-memory chat state: it must not reach a saved
// document, and must not follow the user into the next document through the
// app-preference carry-over.

/// Digits unlikely to occur in a saved document by chance.
const MARKER: u64 = 987_654_321;

fn state_with_usage() -> EditorState {
    let mut state = EditorState::new();
    state.chat.record_run_usage(UsageView {
        input: MARKER,
        cache_read: MARKER + 1,
        cache_write: MARKER + 2,
        output: MARKER + 3,
        turns: 4,
    });
    state
}

#[test]
fn usage_is_not_persisted_in_documents_or_settings() {
    let state = state_with_usage();
    assert!(state.chat.last_run_usage.is_some());

    // The real document writer: the file carries nothing of the meter.
    let path = std::env::temp_dir().join(format!(
        "openpencil-usage-persist-{}.op",
        std::process::id()
    ));
    save_to_path(&state, &path).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    for needle in [MARKER, MARKER + 1, MARKER + 2, MARKER + 3] {
        assert!(!saved.contains(&needle.to_string()), "saved: {saved}");
    }
    assert!(!saved.contains("last_run_usage") && !saved.contains("lastRunUsage"));

    // The settings path: what follows the user into the next document (and
    // what is written to the app's preference files) is `ChatAppPrefs`, which
    // does not see the meter -- two states differing only in usage are equal.
    let mut without = state.clone();
    without.chat.last_run_usage = None;
    assert_eq!(state.chat.app_prefs(), without.chat.app_prefs());

    // Opening or creating a document keeps the user's preferences but not
    // the previous document's run.
    let mut next = EditorState::new();
    preserve_app_preferences(&state, &mut next);
    assert_eq!(next.chat.last_run_usage, None);
}
