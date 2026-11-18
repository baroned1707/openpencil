//! Style suggestions: the gates, the ranking, the debounce and the dismiss.
//!
//! Every "no chips" test first proves the SAME prompt does produce chips with
//! the gate open, so a gate that is simply broken cannot pass them vacuously.

use super::*;
use op_editor_core::EditorState;

const FINTECH: &str = "design a fintech dashboard for a small bank";

/// An empty canvas with `text` typed into the composer.
fn typed(text: &str) -> EditorState {
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state.chat.set_input_text(text.to_string());
    state
}

/// Tick until the debounce has elapsed once; returns the time it elapsed at.
fn settle(driver: &mut StyleSuggestDriver, state: &mut EditorState, start_ms: u64) -> u64 {
    driver.tick(state, start_ms);
    let done = start_ms + DEBOUNCE_MS;
    driver.tick(state, done);
    done
}

fn existing_screen() -> jian_ops_schema::node::PenNode {
    serde_json::from_value(serde_json::json!({
        "type": "frame", "id": "home", "name": "Home", "width": 390, "height": 844,
        "children": [{ "type": "text", "id": "title", "content": "Welcome" }]
    }))
    .expect("screen fixture")
}

fn has_style_table() -> bool {
    op_ai_skills::style_by_ref("glassmorphism").is_some()
}

#[test]
fn suggestions_appear_after_the_debounce_for_a_new_design_prompt() {
    if !has_style_table() {
        return;
    }
    let mut state = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    assert!(
        !driver.tick(&mut state, 1_000),
        "first sight only starts the wait"
    );
    assert!(state.chat.style_suggestions.is_empty());
    assert_eq!(driver.next_deadline_ms(), Some(1_000 + DEBOUNCE_MS));
    assert!(
        !driver.tick(&mut state, 1_000 + DEBOUNCE_MS - 1),
        "still waiting"
    );
    assert!(state.chat.style_suggestions.is_empty());

    assert!(
        driver.tick(&mut state, 1_000 + DEBOUNCE_MS),
        "the wait is over"
    );
    let chips = &state.chat.style_suggestions;
    assert!((1..=3).contains(&chips.len()), "{chips:?}");
    for chip in chips {
        assert!(!chip.name.is_empty());
        assert!(chip.blurb.chars().count() <= op_editor_core::STYLE_BLURB_MAX_CHARS);
    }
    assert_eq!(driver.next_deadline_ms(), None, "nothing left to wake for");
}

#[test]
fn typing_does_not_search_every_frame() {
    if !has_style_table() {
        return;
    }
    let mut state = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    // 200 frames of an unchanged text: one search.
    for frame in 0..200 {
        driver.tick(&mut state, frame * 16);
    }
    assert_eq!(driver.searches, 1);

    // Typing: a new character every 40 ms never lets the text sit still.
    let mut now = 200 * 16;
    for index in 0..30 {
        state.chat.set_input_text(format!("{FINTECH} v{index}"));
        now += 40;
        driver.tick(&mut state, now);
    }
    assert_eq!(
        driver.searches, 1,
        "no search while the text keeps changing"
    );
    // The pause after the last key: exactly one more.
    for extra in 1..=40 {
        driver.tick(&mut state, now + extra * 16);
    }
    assert_eq!(driver.searches, 2);
}

#[test]
fn no_suggestions_when_a_style_is_pinned() {
    if !has_style_table() {
        return;
    }
    // Control: the same prompt with the gate open produces chips.
    let mut open = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    settle(&mut driver, &mut open, 0);
    assert!(!open.chat.style_suggestions.is_empty(), "control");

    let mut pinned = typed(FINTECH);
    pinned.editor_ui.pinned_style_guide = Some("dark-bold-mobile".into());
    let mut driver = StyleSuggestDriver::default();
    settle(&mut driver, &mut pinned, 0);
    assert!(pinned.chat.style_suggestions.is_empty());
    assert_eq!(driver.searches, 0, "a hard gate never searches");

    // Pinning while chips are showing clears them (and the pick) at once.
    let mut live = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    settle(&mut driver, &mut live, 0);
    live.chat.toggle_chosen_style(0);
    assert!(live.chat.has_style_suggest_row());
    live.editor_ui.pinned_style_guide = Some("dark-bold-mobile".into());
    assert!(driver.tick(&mut live, 400), "the row changed");
    assert!(!live.chat.has_style_suggest_row());
    assert_eq!(live.chat.chosen_style, None);
}

#[test]
fn no_suggestions_when_design_md_exists() {
    if !has_style_table() {
        return;
    }
    let mut open = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    settle(&mut driver, &mut open, 0);
    assert!(!open.chat.style_suggestions.is_empty(), "control");

    let mut with_md = typed(FINTECH);
    with_md.doc.design_md = Some(jian_ops_schema::DesignMdSpec {
        raw: "Use the brand blue.".into(),
        project_name: None,
        visual_theme: None,
        color_palette: None,
        typography: None,
        component_styles: None,
        layout_principles: None,
        generation_notes: None,
    });
    let mut driver = StyleSuggestDriver::default();
    settle(&mut driver, &mut with_md, 0);
    assert!(with_md.chat.style_suggestions.is_empty());
    assert_eq!(driver.searches, 0);
}

#[test]
fn no_suggestions_for_short_or_modify_text() {
    if !has_style_table() {
        return;
    }
    assert!(
        !suggest_styles(&typed(FINTECH), FINTECH).is_empty(),
        "control"
    );
    let state = typed("");
    for text in [
        "design it",                                     // too short
        "make the dashboard header blue and bigger",     // modify
        "change the fintech dashboard colors to purple", // modify
        "what is a fintech dashboard design",            // question
        "thanks, that looks good to me",                 // not a design request
        "!!! ???",                                       // no content at all
    ] {
        assert!(suggest_styles(&state, text).is_empty(), "{text:?}");
    }
    // CJK: six characters are enough; five are not.
    assert!(is_long_enough("设计一个银行"), "six characters are enough");
    assert!(!is_long_enough("设计一个银"), "five are not");
}

#[test]
fn the_length_gate_counts_words_and_cjk_characters() {
    assert!(is_long_enough("design a fintech dashboard"));
    assert!(!is_long_enough("fintech dashboard"), "two words");
    assert!(!is_long_enough("a b c d"), "under twelve characters");
    assert!(is_long_enough("设计一个理发店管理网站"));
}

#[test]
fn an_existing_screen_needs_a_new_screen_request() {
    if !has_style_table() {
        return;
    }
    let mut state = typed("");
    state.active_children_mut().push(existing_screen());
    assert!(
        suggest_styles(&state, FINTECH).is_empty(),
        "styles do not suit work on a screen that exists"
    );
    // Control: the same canvas with a prompt that asks for a new screen.
    let new_screen = "make a new fintech dashboard screen for a small bank";
    assert!(!suggest_styles(&state, new_screen).is_empty());
    // And the empty canvas takes the plain prompt.
    assert!(!suggest_styles(&typed(""), FINTECH).is_empty());
}

#[test]
fn weak_match_shows_no_chips() {
    if !has_style_table() {
        return;
    }
    let state = typed("");
    assert!(!suggest_styles(&state, FINTECH).is_empty(), "control");
    // Design wording that matches no product type and names no style.
    for text in [
        "design something nice for my friend",
        "create a login page for the app please",
    ] {
        assert!(suggest_styles(&state, text).is_empty(), "{text:?}");
    }
}

#[test]
fn at_most_three_distinct_resolved_styles() {
    if !has_style_table() {
        return;
    }
    let text = "a sleek dark glassmorphism fintech dashboard for crypto trading, design it";
    let chips = suggest_styles(&typed(""), text);
    assert!((1..=3).contains(&chips.len()), "{chips:?}");
    let mut seen = std::collections::HashSet::new();
    for chip in &chips {
        assert!(
            seen.insert(chip.style_ref.clone()),
            "duplicate {}",
            chip.style_ref
        );
        let record = op_ai_skills::style_by_ref(&chip.style_ref).expect("every chip resolves");
        assert_eq!(record.name, chip.name);
    }
    assert_eq!(
        chips[0].style_ref, "glassmorphism",
        "a style named in the text leads"
    );
}

#[test]
fn dismiss_hides_until_the_text_changes() {
    if !has_style_table() {
        return;
    }
    let mut state = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    let now = settle(&mut driver, &mut state, 0);
    assert!(state.chat.has_style_suggest_row(), "control");

    assert!(state.chat.dismiss_style_suggestions());
    for frame in 1..=60 {
        driver.tick(&mut state, now + frame * 16);
    }
    assert!(
        !state.chat.has_style_suggest_row(),
        "stays hidden for the same text"
    );
    assert_eq!(driver.searches, 1, "and nothing is searched again");

    state
        .chat
        .set_input_text(format!("{FINTECH} with dark mode"));
    settle(&mut driver, &mut state, now + 2_000);
    assert!(
        state.chat.has_style_suggest_row(),
        "new text brings the row back"
    );
}

#[test]
fn a_pick_survives_new_chips_and_empties_with_the_composer() {
    if !has_style_table() {
        return;
    }
    let mut state = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    let now = settle(&mut driver, &mut state, 0);
    state.chat.toggle_chosen_style(0);
    let picked = state.chat.chosen_style.clone().expect("picked");

    state
        .chat
        .set_input_text(format!("{FINTECH} with dark mode"));
    settle(&mut driver, &mut state, now + 1_000);
    assert_eq!(
        state.chat.chosen_style,
        Some(picked),
        "retyping keeps the pick"
    );

    state.chat.set_input_text("");
    assert!(driver.tick(&mut state, now + 5_000), "the row changed");
    assert_eq!(
        state.chat.chosen_style, None,
        "an empty composer forgets it"
    );
    assert!(!state.chat.has_style_suggest_row());
}

#[test]
fn a_queued_send_keeps_the_pick_for_the_launcher() {
    if !has_style_table() {
        return;
    }
    let mut state = typed(FINTECH);
    let mut driver = StyleSuggestDriver::default();
    let now = settle(&mut driver, &mut state, 0);
    state.chat.toggle_chosen_style(0);
    assert!(
        state.chat.begin_send(),
        "the composer is now empty and queued"
    );
    assert!(!driver.tick(&mut state, now + 100));
    assert!(
        state.chat.chosen_style.is_some(),
        "the launcher, not the driver, consumes the pick"
    );
}

#[test]
fn the_frame_loop_runs_the_debounce_and_asks_for_its_wake() {
    // An idle app has no other frame coming after the last keystroke: the
    // chips only appear if `prepare_redraw` ticks the driver AND the loop is
    // told to wake when the pause ends.
    if !has_style_table() {
        return;
    }
    let mut app = crate::DesktopApp::new(None);
    app.host
        .editor_state_mut()
        .chat
        .set_input_text(FINTECH.to_string());

    app.prepare_redraw();
    assert!(app.host.editor_state().chat.style_suggestions.is_empty());
    assert!(app.style_suggest_wake().is_some(), "a wake is owed");
    assert!(
        app.resume_time_needs_redraw(),
        "and the timed wake must repaint"
    );

    // The pause elapses with no other event: the wake's redraw offers chips.
    app.clock_start -= std::time::Duration::from_millis(DEBOUNCE_MS + 50);
    assert!(app.prepare_redraw(), "the row changed, so the frame paints");
    let chips = &app.host.editor_state().chat.style_suggestions;
    assert!((1..=3).contains(&chips.len()), "{chips:?}");
    assert_eq!(app.style_suggest_wake(), None, "nothing left to wait for");
}
