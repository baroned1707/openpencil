use super::*;
use op_editor_core::editor_ui_state::FileAction;

fn host_with(children: serde_json::Value) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().doc = serde_json::from_value(serde_json::json!({
        "version": "1.0.0", "children": children
    }))
    .expect("fixture document");
    host
}

fn credited_image(id: &str, creator: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "image", "id": id, "name": id, "width": 10, "height": 10,
        "src": format!("https://x/{id}.png"),
        "imageCredit": {
            "provider": "openverse", "creator": creator, "license": "CC BY 4.0",
            "licenseUrl": "https://creativecommons.org/licenses/by/4.0/",
            "sourcePageUrl": format!("https://openverse.org/image/{id}")
        }
    })
}

#[test]
fn copy_image_credits_menu_copies_text_and_toasts() {
    let mut host = host_with(serde_json::json!([
        credited_image("hero", "Jane Doe"),
        credited_image("card", "Bob")
    ]));
    // The File menu row raises exactly this action; run it through the same
    // dispatcher the menu uses.
    let mut current_path = None;
    crate::persistence::run_action(
        FileAction::CopyImageCredits,
        &mut host,
        &mut current_path,
        None,
    );

    let state = host.editor_state();
    let text = state
        .chat
        .pending_copy_text
        .as_deref()
        .expect("copied text");
    assert_eq!(text.lines().count(), 2);
    assert!(
        text.starts_with("\"hero\" by Jane Doe — CC BY 4.0"),
        "{text}"
    );
    assert!(text.contains("credit required"));
    let toast = state.editor_ui.editor_toast.as_ref().expect("toast");
    assert_eq!(toast.i18n_key, COPIED_KEY);
    assert_eq!(toast.args, vec![("count".to_string(), "2".to_string())]);
    // The shipped English wording interpolates the count.
    let sentence = op_i18n::interpolate(
        op_i18n::translate(state.editor_ui.locale, "imageCredits.copied"),
        &toast.arg_pairs(),
    );
    assert_eq!(sentence, "2 image credits copied");
}

#[test]
fn one_credit_uses_the_singular_toast_and_none_leaves_the_clipboard_alone() {
    let mut host = host_with(serde_json::json!([credited_image("hero", "Jane Doe")]));
    copy_image_credits(&mut host);
    let toast = host
        .editor_state()
        .editor_ui
        .editor_toast
        .clone()
        .expect("toast");
    assert_eq!(toast.i18n_key, COPIED_ONE_KEY);
    assert!(toast.args.is_empty());

    let mut empty = host_with(serde_json::json!([
        {"type": "image", "id": "a", "src": "https://x/a.png", "width": 1, "height": 1}
    ]));
    copy_image_credits(&mut empty);
    let state = empty.editor_state();
    assert!(state.chat.pending_copy_text.is_none());
    let toast = state.editor_ui.editor_toast.as_ref().expect("toast");
    assert_eq!(toast.i18n_key, NONE_KEY);
    assert_eq!(
        op_i18n::translate(state.editor_ui.locale, "imageCredits.none"),
        "No image credits in this document"
    );
}
