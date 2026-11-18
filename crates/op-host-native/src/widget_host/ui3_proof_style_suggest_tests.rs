//! Proof scenes for the style suggestions above the composer: three chips
//! under a typed prompt, one chosen, none, and the hover card.

use crate::widget_host::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::chat::{AgentProvider, ModelEntry};
use op_editor_core::{style_blurb, ChatFooterButton, StyleSuggestion, ThemeMode};

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn suggestion(style_ref: &str, name: &str, best_for: &str) -> StyleSuggestion {
    StyleSuggestion {
        style_ref: style_ref.to_string(),
        name: name.to_string(),
        blurb: style_blurb(best_for),
    }
}

/// What the desktop offers for a fintech dashboard prompt (real records, long
/// names: only two fit the default rail's row at a readable width).
fn fintech_chips() -> Vec<StyleSuggestion> {
    vec![
        suggestion(
            "dark-mode-oled",
            "Dark Mode (OLED)",
            "Night-mode apps, coding platforms, entertainment, eye-strain prevention, OLED devices, low-light",
        ),
        suggestion(
            "data-dense-dashboard",
            "Data-Dense Dashboard",
            "Business intelligence dashboards, financial analytics, enterprise reporting, operational dashboards, data warehousing",
        ),
        suggestion(
            "minimalism-and-swiss-style",
            "Minimalism & Swiss Style",
            "Enterprise apps, dashboards, documentation sites, SaaS platforms, professional tools",
        ),
    ]
}

/// What the desktop offers for an indie-game portfolio prompt (real records,
/// short names: all three fit a wide rail beside the label).
fn portfolio_chips() -> Vec<StyleSuggestion> {
    vec![
        suggestion(
            "pixel-art",
            "Pixel Art",
            "Indie games, retro tools, creative portfolios, nostalgia marketing, Easter eggs",
        ),
        suggestion(
            "brutalism",
            "Brutalism",
            "Design portfolios, artistic projects, counter-culture brands",
        ),
        suggestion(
            "vaporwave",
            "Vaporwave",
            "Music platforms, gaming, creative portfolios, tech startups",
        ),
    ]
}

/// The Agent tab with a design prompt typed and `chips` offered.
fn typing_host(theme: ThemeMode, prompt: &str, chips: Vec<StyleSuggestion>) -> WidgetHostNative {
    let mut host = editor_host(theme, None);
    let state = host.editor_state_mut();
    state.editor_ui.enter_chat_tab();
    state.chat.available_models = vec![ModelEntry::new(
        AgentProvider::ClaudeCode,
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
    )];
    state.chat.selected_model = 0;
    state.chat.set_input_text(prompt.to_string());
    state.chat.focused = true;
    if !chips.is_empty() {
        state.chat.set_style_suggestions(1, chips);
    }
    host
}

const FINTECH: &str = "design a fintech dashboard for a small bank";
const PORTFOLIO: &str = "design a retro game portfolio site for an indie studio";

/// A rail dragged wider (the rail is the chat's column), where the label and
/// all three chips fit.
fn widen(host: &mut WidgetHostNative) {
    host.editor_state_mut()
        .editor_ui
        .set_layer_panel_width(480.0);
}

fn render_both(
    name: &str,
    prompt: &str,
    chips: fn() -> Vec<StyleSuggestion>,
    build: impl Fn(&mut WidgetHostNative),
) {
    for theme in THEMES {
        let mut host = typing_host(theme, prompt, chips());
        build(&mut host);
        let rgb = render_scene(&mut host, name, (60, 400));
        assert!(rgb != [0, 0, 0], "{theme:?}: {name} painted something");
    }
}

#[test]
fn style_suggest_three_chips_render() {
    render_both("99-style-suggest-three", PORTFOLIO, portfolio_chips, widen);
}

#[test]
fn style_suggest_chosen_renders() {
    render_both(
        "99-style-suggest-chosen",
        PORTFOLIO,
        portfolio_chips,
        |host| {
            widen(host);
            host.editor_state_mut().chat.toggle_chosen_style(1);
        },
    );
}

#[test]
fn style_suggest_default_rail_renders() {
    render_both("99-style-suggest-rail", FINTECH, fintech_chips, |_| {});
}

#[test]
fn style_suggest_hover_card_renders() {
    render_both("99-style-suggest-hover", FINTECH, fintech_chips, |host| {
        host.editor_state_mut().editor_ui.chat_footer_hover =
            Some(ChatFooterButton::StyleSuggestChip(1));
    });
}

#[test]
fn style_suggest_none_renders() {
    for theme in THEMES {
        let mut host = typing_host(theme, FINTECH, Vec::new());
        let rgb = render_scene(&mut host, "99-style-suggest-none", (60, 400));
        assert!(rgb != [0, 0, 0], "{theme:?}: painted something");
    }
}
