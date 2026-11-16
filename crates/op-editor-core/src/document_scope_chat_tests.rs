//! The chat belongs to its document; the user's model choice does not.

use super::*;
use crate::chat::{AgentProvider, ChatMessage, ModelEntry};

fn with_models(state: &mut EditorState, names: &[&str], selected: usize) {
    state.chat.available_models = names
        .iter()
        .map(|name| ModelEntry::new(AgentProvider::ClaudeCode, *name, *name))
        .collect();
    state.chat.selected_model = selected;
}

#[test]
fn chat_tabs_are_per_document_and_app_prefs_follow_the_user() {
    let mut live = EditorState::starter();
    live.chat.messages.push(ChatMessage::user("design a login"));
    live.chat.new_tab();
    live.chat.messages.push(ChatMessage::user("second tab"));
    with_models(&mut live, &["a", "b"], 1);
    live.chat.panel_width = 777.0;
    live.chat.maximized = true;

    let mut parked = DocumentScope::from_state(EditorState::starter());
    live.switch_document_scope(&mut parked);

    // The arriving document has its own empty conversation...
    assert_eq!(live.chat.tab_count(), 1);
    assert!(live.chat.messages.is_empty());
    // ...but the user's models and panel placement came along.
    assert_eq!(live.chat.available_models.len(), 2);
    assert_eq!(live.chat.selected_model, 1);
    assert_eq!(live.chat.panel_width, 777.0);
    assert!(live.chat.maximized);

    // A choice made here is what the user finds when they go back.
    live.chat.selected_model = 0;
    live.chat.messages.push(ChatMessage::user("only in B"));
    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.tab_count(), 2, "A keeps both of its tabs");
    assert_eq!(
        live.chat.selected_model, 0,
        "the newer choice follows the user"
    );
    assert_eq!(live.chat.tabs()[0].messages.len(), 1);
    assert_eq!(live.chat.tabs()[0].messages[0].content, "design a login");
    assert_eq!(live.chat.tabs()[1].messages[0].content, "second tab");
    assert!(
        live.chat
            .tabs()
            .iter()
            .all(|tab| tab.available_models.len() == 2),
        "every tab of the arriving document gets the models"
    );
}

#[test]
fn the_pure_exchange_moves_the_chat_untouched() {
    let mut live = EditorState::starter();
    live.chat.messages.push(ChatMessage::user("mine"));
    with_models(&mut live, &["a"], 0);

    let mut other = EditorState::starter();
    with_models(&mut other, &["stale"], 0);
    let mut parked = DocumentScope::from_state(other);
    live.exchange_document_scope(&mut parked);
    assert_eq!(live.chat.available_models[0].value, "stale");
    assert!(live.chat.messages.is_empty());

    live.exchange_document_scope(&mut parked);
    assert_eq!(live.chat.available_models[0].value, "a");
    assert_eq!(live.chat.messages[0].content, "mine");
}

#[test]
fn a_scope_with_a_used_chat_is_not_pristine() {
    let fresh = DocumentScope::from_state(EditorState::starter());
    assert!(fresh.is_pristine());

    let mut chatted = EditorState::starter();
    chatted.chat.messages.push(ChatMessage::user("hello"));
    assert!(!DocumentScope::from_state(chatted).is_pristine());

    let mut drafting = EditorState::starter();
    drafting.chat.input.set_text("half a thought");
    assert!(!DocumentScope::from_state(drafting).is_pristine());

    let mut two_tabs = EditorState::starter();
    two_tabs.chat.new_tab();
    assert!(!DocumentScope::from_state(two_tabs).is_pristine());
}
