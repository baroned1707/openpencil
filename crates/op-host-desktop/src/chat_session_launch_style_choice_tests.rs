//! The style a user picked from the composer's suggestions lives for ONE run:
//! the launcher reads it into the request, then forgets it.

use super::*;
use op_editor_core::StyleSuggestion;

fn glass() -> StyleSuggestion {
    StyleSuggestion {
        style_ref: "glassmorphism".into(),
        name: "Glassmorphism".into(),
        blurb: "Modern SaaS".into(),
    }
}

/// Send `text` on a fresh CLI-routed host (the default selection) and return
/// the request JSON stashed on the turn's assistant bubble.
fn send_and_read_request(host: &mut WidgetHostNative, text: &str) -> String {
    host.editor_state_mut().chat.set_input_text(text);
    assert!(host.editor_state_mut().chat.begin_send());
    let mut current_chat = None;
    let mut current_design = None;
    assert!(launch_if_pending(
        host,
        &mut current_chat,
        &mut current_design
    ));
    host.editor_state()
        .chat
        .messages
        .last()
        .and_then(|msg| msg.design_request_json_for_retry.clone())
        .expect("the CLI route stashes the request")
}

#[test]
fn choice_is_cleared_after_one_run() {
    let _guard = crate::agent_indicator_test_lock::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    op_editor_core::agent_indicators::clear();
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().chat.chosen_style = Some(glass());

    let first = send_and_read_request(&mut host, "design a fintech dashboard for a bank");
    assert_eq!(
        host.editor_state().chat.chosen_style,
        None,
        "the launcher consumed the choice"
    );
    if op_ai_skills::style_by_ref("glassmorphism").is_some() {
        assert!(first.contains("styleHint"), "the run carried it: {first}");
        assert!(first.contains("Glassmorphism"));
    }

    // The next run starts clean: no hint, byte for byte the request without one.
    let second = send_and_read_request(&mut host, "design a fintech dashboard for a bank");
    assert!(!second.contains("styleHint"), "{second}");
    op_editor_core::agent_indicators::clear();
}

#[test]
fn a_choice_is_dropped_even_when_the_run_takes_another_route() {
    // No design request is built when the transport cannot start; the pick
    // must still not leak into the next, unrelated send.
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().chat.chosen_style = Some(glass());
    host.editor_state_mut().chat.set_input_text("thanks");
    assert!(host.editor_state_mut().chat.begin_send());
    let (mut chat, mut design) = (None, None);
    let _ = launch_if_pending(&mut host, &mut chat, &mut design);
    assert_eq!(host.editor_state().chat.chosen_style, None);
    op_editor_core::agent_indicators::clear();
}

#[test]
fn nothing_to_launch_leaves_the_choice_alone() {
    // The launcher runs every frame: it must not eat a pick while no send waits.
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().chat.chosen_style = Some(glass());
    let (mut chat, mut design) = (None, None);
    assert!(!launch_if_pending(&mut host, &mut chat, &mut design));
    assert_eq!(host.editor_state().chat.chosen_style, Some(glass()));
}

#[test]
fn a_pinned_route_send_never_carries_the_composers_pick() {
    // Home and restyle sends are briefs of their own; the composer row is not
    // even on screen then, so a lingering pick must not steer them.
    let _guard = crate::agent_indicator_test_lock::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    op_editor_core::agent_indicators::clear();
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().chat.chosen_style = Some(glass());
    host.editor_state_mut().chat.launch_route = op_editor_core::LaunchRoute::Orchestrator;
    let json = send_and_read_request(&mut host, "design a fintech dashboard for a bank");
    assert!(!json.contains("styleHint"), "{json}");
    assert_eq!(host.editor_state().chat.chosen_style, None);
    op_editor_core::agent_indicators::clear();
}
