use super::*;

fn style(name: &str) -> StyleSuggestion {
    StyleSuggestion {
        style_ref: name.to_lowercase().replace(' ', "-"),
        name: name.to_string(),
        blurb: format!("Best for {name}"),
    }
}

fn three() -> Vec<StyleSuggestion> {
    vec![style("Glassmorphism"), style("Bento Box"), style("Swiss")]
}

#[test]
fn blurb_keeps_a_short_text_whole_and_cuts_a_long_one_at_a_word() {
    assert_eq!(style_blurb("Dashboards, SaaS"), "Dashboards, SaaS");
    // Whitespace is normalised, never padded.
    assert_eq!(style_blurb("  Dash   boards "), "Dash boards");
    let long = "Marketing sites, portfolios, editorial pages, product launches, \
                design agencies and creative studios of every size";
    let blurb = style_blurb(long);
    assert!(blurb.chars().count() <= STYLE_BLURB_MAX_CHARS, "{blurb}");
    assert!(blurb.ends_with('…'));
    let body = blurb.trim_end_matches('…');
    assert!(
        long.starts_with(body) && long[body.len()..].starts_with([' ', ',']),
        "cut at a word boundary, not inside a word: {blurb}"
    );
    // Exactly at the limit: no ellipsis.
    let exact = "a".repeat(STYLE_BLURB_MAX_CHARS);
    assert_eq!(style_blurb(&exact), exact);
}

#[test]
fn a_blurb_without_spaces_is_still_bounded() {
    let blurb = style_blurb(&"x".repeat(200));
    assert_eq!(blurb.chars().count(), STYLE_BLURB_MAX_CHARS);
    assert!(blurb.ends_with('…'));
}

#[test]
fn offers_at_most_three_chips() {
    let mut chat = ChatState::default();
    let mut many = three();
    many.push(style("Fourth"));
    chat.set_style_suggestions(1, many);
    assert_eq!(chat.style_suggestions.len(), MAX_STYLE_SUGGESTIONS);
    assert_eq!(chat.suggestions_for, Some(1));
}

#[test]
fn choosing_a_chip_twice_lets_it_go() {
    let mut chat = ChatState::default();
    chat.set_style_suggestions(1, three());
    assert!(chat.toggle_chosen_style(1));
    assert_eq!(
        chat.chosen_style.as_ref().map(|s| s.name.as_str()),
        Some("Bento Box")
    );
    // Another chip replaces the pick.
    assert!(chat.toggle_chosen_style(2));
    assert_eq!(
        chat.chosen_style.as_ref().map(|s| s.name.as_str()),
        Some("Swiss")
    );
    assert!(chat.toggle_chosen_style(2));
    assert_eq!(chat.chosen_style, None);
    assert!(!chat.toggle_chosen_style(9), "no such chip");
}

#[test]
fn dismiss_hides_until_the_text_changes() {
    let mut chat = ChatState::default();
    chat.set_input_text("a fintech dashboard for small teams".to_string());
    let text = text_fingerprint(&chat.input.effective_text());
    chat.set_style_suggestions(text, three());
    assert!(chat.has_style_suggest_row());

    assert!(chat.dismiss_style_suggestions());
    assert!(!chat.has_style_suggest_row());
    // The same text computed again stays hidden.
    chat.set_style_suggestions(text, three());
    assert!(chat.style_suggestions.is_empty());
    assert!(!chat.has_style_suggest_row());
    // New text lifts the dismissal.
    let changed = text_fingerprint("a fintech dashboard for large teams");
    chat.set_style_suggestions(changed, three());
    assert_eq!(chat.style_suggestions.len(), 3);
    assert_eq!(chat.suggestions_dismissed_for, None);
}

#[test]
fn a_choice_keeps_the_row_visible_and_dismiss_forgets_it() {
    let mut chat = ChatState::default();
    chat.set_style_suggestions(1, three());
    chat.toggle_chosen_style(0);
    // The offered list is replaced by one without the pick: the pick is still
    // shown (alone), because it will apply to the next run.
    chat.set_style_suggestions(2, vec![style("Other")]);
    assert!(chat.has_style_suggest_row());
    let chips = chat.style_suggest_chips();
    assert_eq!(chips.len(), 1);
    assert_eq!(chips[0].name, "Glassmorphism");
    // Even with no list at all.
    chat.clear_style_suggestions();
    assert!(chat.has_style_suggest_row());
    chat.dismiss_style_suggestions();
    assert_eq!(chat.chosen_style, None);
    assert!(!chat.has_style_suggest_row());
}

#[test]
fn a_run_clears_the_chips_but_keeps_the_choice_for_the_launcher() {
    let mut chat = ChatState::default();
    chat.set_input_text("design a landing page for a coffee shop".to_string());
    chat.set_style_suggestions(1, three());
    chat.toggle_chosen_style(0);
    assert!(chat.begin_send());
    assert!(chat.style_suggestions.is_empty());
    assert_eq!(chat.suggestions_for, None);
    assert!(
        chat.chosen_style.is_some(),
        "the launcher consumes the choice, not begin_send"
    );
    chat.new_chat();
    assert_eq!(
        chat.chosen_style, None,
        "a fresh chat forgets a stale choice"
    );
}

#[test]
fn clicking_the_lone_pick_lets_it_go_instead_of_picking_another() {
    let mut chat = ChatState::default();
    chat.set_style_suggestions(1, three());
    chat.toggle_chosen_style(2);
    // The list is replaced by one without the pick: the row shows the pick
    // alone, so chip 0 is the pick and clicking it releases it.
    chat.set_style_suggestions(2, vec![style("Other"), style("Another")]);
    assert_eq!(chat.style_suggest_chips().len(), 1);
    assert!(chat.toggle_chosen_style(0));
    assert_eq!(chat.chosen_style, None);
}
