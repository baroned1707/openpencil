//! Proof scenes for the chat usage meter: a finished run with cache data,
//! one without, none at all, and the hover card.

use crate::widget_host::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::chat::{AgentProvider, ChatMessage, ModelEntry};
use op_editor_core::{ChatFooterButton, ThemeMode, UsageView};

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// A finished design turn on the rail's Agent tab, with a model chip.
fn finished_run_host(theme: ThemeMode, usage: Option<UsageView>) -> WidgetHostNative {
    let mut host = editor_host(theme, None);
    let state = host.editor_state_mut();
    state.editor_ui.enter_chat_tab();
    state.chat.available_models = vec![ModelEntry::new(
        AgentProvider::ClaudeCode,
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
    )];
    state.chat.selected_model = 0;
    state.chat.messages.push(ChatMessage::user(
        "Design a dashboard card with a metric and a primary button.",
    ));
    let mut assistant = ChatMessage::assistant(
        "Built the dashboard card with a headline metric, a trend line and a primary button. \
         Spacing follows an 8 px grid and the button text passes contrast.",
    );
    assistant.agent_name = Some("Claude Code".to_string());
    state.chat.messages.push(assistant);
    state.chat.last_run_usage = usage;
    host
}

fn render_both(name: &str, usage: Option<UsageView>, hover: bool) {
    for theme in THEMES {
        let mut host = finished_run_host(theme, usage);
        if hover {
            host.editor_state_mut().editor_ui.chat_footer_hover =
                Some(ChatFooterButton::UsageMeter);
        }
        let rgb = render_scene(&mut host, name, (60, 400));
        assert!(rgb != [0, 0, 0], "{theme:?}: {name} painted something");
    }
}

const WITH_CACHE: UsageView = UsageView {
    input: 4_000,
    cache_read: 41_000,
    cache_write: 3_200,
    output: 3_100,
    turns: 6,
};
const NO_CACHE: UsageView = UsageView {
    input: 48_200,
    cache_read: 0,
    cache_write: 0,
    output: 3_100,
    turns: 6,
};

#[test]
fn usage_meter_with_cache_renders() {
    render_both("98-usage-cache", Some(WITH_CACHE), false);
}

#[test]
fn usage_meter_without_cache_renders() {
    render_both("98-usage-nocache", Some(NO_CACHE), false);
}

#[test]
fn usage_meter_hidden_renders() {
    render_both("98-usage-none", None, false);
}

#[test]
fn usage_meter_hover_card_renders() {
    render_both("98-usage-hover", Some(WITH_CACHE), true);
}
