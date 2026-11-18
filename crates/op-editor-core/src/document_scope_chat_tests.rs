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

#[test]
fn usage_follows_the_document_tab() {
    // The usage meter belongs to the conversation, so it travels with the
    // document's chat tab: a run in A is not shown in B, and is found again
    // when the user comes back.
    let usage = crate::UsageView {
        input: 100,
        cache_read: 900,
        cache_write: 0,
        output: 40,
        turns: 3,
    };
    let mut live = EditorState::starter();
    live.chat.messages.push(ChatMessage::user("design a login"));
    live.chat.record_run_usage(usage);
    live.chat.new_tab();
    assert_eq!(
        live.chat.last_run_usage, None,
        "a second tab has its own meter"
    );

    let mut parked = DocumentScope::from_state(EditorState::starter());
    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.last_run_usage, None, "document B has no run yet");
    let other = crate::UsageView { output: 7, ..usage };
    live.chat.record_run_usage(other);

    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.tab_count(), 2);
    assert_eq!(live.chat.tabs()[0].last_run_usage, Some(usage));
    assert_eq!(live.chat.tabs()[1].last_run_usage, None);

    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.last_run_usage, Some(other), "B keeps its own");
}

fn suggestion(name: &str) -> crate::StyleSuggestion {
    crate::StyleSuggestion {
        style_ref: name.to_lowercase(),
        name: name.to_string(),
        blurb: format!("Best for {name}"),
    }
}

#[test]
fn style_suggestions_follow_the_document_tab() {
    // The chips describe the draft typed in THIS tab of THIS document: they
    // travel with it and never show over another document's composer.
    let mut live = EditorState::starter();
    live.chat.set_style_suggestions(7, vec![suggestion("Glass")]);
    live.chat.toggle_chosen_style(0);
    live.chat.new_tab();
    assert!(!live.chat.has_style_suggest_row(), "a second tab starts clean");

    let mut parked = DocumentScope::from_state(EditorState::starter());
    live.switch_document_scope(&mut parked);
    assert!(!live.chat.has_style_suggest_row(), "document B has no row");
    live.chat.set_style_suggestions(8, vec![suggestion("Swiss")]);

    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.tabs()[0].chosen_style, Some(suggestion("Glass")));
    assert_eq!(live.chat.tabs()[0].suggestions_for, Some(7));
    assert!(live.chat.tabs()[1].style_suggestions.is_empty());
    live.switch_document_scope(&mut parked);
    assert_eq!(live.chat.style_suggestions, vec![suggestion("Swiss")]);
}

#[test]
fn suggestions_are_not_persisted() {
    // App-level chat prefs (what settings keep) carry none of the row: a new
    // tab that receives them starts without chips or a choice.
    let mut live = EditorState::starter();
    live.chat.set_style_suggestions(7, vec![suggestion("Glass")]);
    live.chat.toggle_chosen_style(0);
    let prefs = live.chat.app_prefs();

    let mut other = crate::chat_sessions::ChatSessions::default();
    other.apply_app_prefs(&prefs);
    let tab = other.active();
    assert!(tab.style_suggestions.is_empty());
    assert_eq!(tab.chosen_style, None);
    assert_eq!(tab.suggestions_for, None);
    assert_eq!(tab.suggestions_dismissed_for, None);
}
