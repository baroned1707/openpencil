//! The style-suggestion row above the composer: what paints, what hides it,
//! that it costs no layout while absent, how it is pressed and hovered.

use super::tests::PanelPaintBackend;
use super::tests_paint::rect_close;
use super::*;
use crate::widgets::ai_chat_chip_row::CHIP_ROW_HEIGHT;
use crate::widgets::ai_chat_hit::AIChatHit;
use crate::widgets::ai_chat_style_suggest::SuggestRowLayout;
use crate::widgets::chat_click_flow::{apply_chat_hit, ChatClickStep};
use op_editor_core::{ChatFooterButton, ChatMessage, StyleSuggestion};

fn style(name: &str, blurb: &str) -> StyleSuggestion {
    StyleSuggestion {
        style_ref: name.to_lowercase().replace(' ', "-"),
        name: name.to_string(),
        blurb: blurb.to_string(),
    }
}

fn short() -> Vec<StyleSuggestion> {
    vec![
        style("Glass", "Modern SaaS"),
        style("Bento", "Dashboards"),
        style("Swiss", "Corporate sites"),
    ]
}

fn three() -> Vec<StyleSuggestion> {
    vec![
        style("Glassmorphism", "Modern SaaS, financial dashboards"),
        style("Bento Box Grid", "Dashboards, product pages"),
        style("Swiss", "Corporate sites"),
    ]
}

/// The Agent tab with a design prompt typed and `chips` offered.
fn typing(chips: Vec<StyleSuggestion>) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.enter_chat_tab();
    state.chat.messages.push(ChatMessage::user("design a page"));
    state.chat.messages.push(ChatMessage::assistant("Done."));
    state
        .chat
        .set_input_text("a fintech dashboard for a bank".to_string());
    if !chips.is_empty() {
        state.chat.set_style_suggestions(1, chips);
    }
    state
}

fn panel_rect() -> Rect {
    Rect::xywh(0.0, 0.0, AI_CHAT_WIDTH, AI_CHAT_HEIGHT)
}

fn paint(state: &EditorState) -> PanelPaintBackend {
    let panel = AIChatPlaceholder::from_editor(state);
    let mut backend = PanelPaintBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    panel.paint(&mut cx, panel_rect());
    backend
}

fn has_text(backend: &PanelPaintBackend, text: &str) -> bool {
    backend.texts.iter().any(|(t, ..)| t == text)
}

fn layout(state: &EditorState) -> SuggestRowLayout {
    let panel = AIChatPlaceholder::from_editor(state);
    panel
        .style_suggest_layout(panel.input_rows_rect(panel_rect()))
        .expect("the row is showing")
}

fn centre(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn hidden_row_is_zero_height_and_moves_nothing() {
    let without = typing(Vec::new());
    let with = typing(three());
    let (a, b) = (
        AIChatPlaceholder::from_editor(&without),
        AIChatPlaceholder::from_editor(&with),
    );
    let rect = panel_rect();
    assert_eq!(a.style_suggest_row_h(), 0.0);
    assert!(a.style_suggest_layout(a.input_rows_rect(rect)).is_none());
    assert!(
        (b.input_height_for_rect(rect) - a.input_height_for_rect(rect) - CHIP_ROW_HEIGHT).abs()
            < 1e-3,
        "the row adds exactly its own height"
    );
    // The row sits inside the input block, so the text area moves down by
    // the row height and the composer's bottom edge stays where it was.
    assert!(
        (a.input_text_rect(rect).origin.y + CHIP_ROW_HEIGHT - b.input_text_rect(rect).origin.y)
            .abs()
            > 1e-3
    );
    let input_bottom =
        |p: &AIChatPlaceholder| p.input_rect(rect).origin.y + p.input_rect(rect).size.y;
    assert!((input_bottom(&a) - input_bottom(&b)).abs() < 1e-3);
    // Absent: nothing painted that mentions styles.
    let painted = paint(&without);
    assert!(!has_text(&painted, "Suggested style"));
    assert!(!painted.texts.iter().any(|(t, ..)| t == "Glassmorphism"));
}

#[test]
fn the_row_paints_a_label_three_chips_and_a_dismiss() {
    let state = typing(vec![
        style("Glass", "Modern SaaS"),
        style("Bento", "Dashboards"),
        style("Swiss", "Corporate sites"),
    ]);
    let backend = paint(&state);
    assert!(has_text(&backend, "Suggested style"), "{:?}", backend.texts);
    for name in ["Glass", "Bento", "Swiss"] {
        assert!(has_text(&backend, name), "{name}: {:?}", backend.texts);
    }
    assert!(has_text(&backend, "×"));
    // Square chrome, no outlines: the row adds fills only.
    let plain = paint(&typing(Vec::new()));
    assert_eq!(backend.stroke_lines, plain.stroke_lines);
    assert_eq!(backend.round_rects.len(), plain.round_rects.len() + 3);
    // The chips sit directly above the text area.
    let panel = AIChatPlaceholder::from_editor(&state);
    let chips = layout(&state).chips;
    assert_eq!(chips.len(), 3);
    let text_top = panel.input_text_rect(panel_rect()).origin.y;
    for chip in &chips {
        assert!(chip.origin.y + chip.size.y <= text_top);
    }
}

#[test]
fn a_chosen_chip_takes_the_selected_tone_and_the_label_names_it() {
    let mut state = typing(short());
    let resting = paint(&state);
    state.chat.toggle_chosen_style(1);
    let chosen = paint(&state);
    assert!(has_text(&chosen, "Style: Bento"), "{:?}", chosen.texts);
    assert!(!has_text(&chosen, "Suggested style"));
    let theme = Theme::dark();
    let tone =
        |b: &PanelPaintBackend, color| b.round_rects.iter().filter(|(_, _, c)| *c == color).count();
    assert_eq!(
        tone(&chosen, theme.row_selected),
        tone(&resting, theme.row_selected) + 1,
        "exactly one chip is in the selected tone"
    );
    // Picking it again lets it go.
    state.chat.toggle_chosen_style(1);
    assert!(has_text(&paint(&state), "Suggested style"));
}

#[test]
fn a_pick_stays_visible_alone_when_the_offered_list_is_gone() {
    let mut state = typing(three());
    state.chat.toggle_chosen_style(0);
    state.chat.clear_style_suggestions();
    let backend = paint(&state);
    assert!(has_text(&backend, "Style: Glassmorphism"));
    assert_eq!(layout(&state).chips.len(), 1);
}

#[test]
fn pressing_a_chip_or_the_dismiss_resolves_to_its_hit() {
    let state = typing(three());
    let panel = AIChatPlaceholder::from_editor(&state);
    let rect = panel_rect();
    let layout = layout(&state);
    for (index, chip) in layout.chips.iter().enumerate() {
        assert_eq!(
            panel.hit_test(rect, centre(*chip)),
            Some(AIChatHit::ToggleSuggestedStyle(index))
        );
    }
    assert_eq!(
        panel.hit_test(rect, centre(layout.dismiss)),
        Some(AIChatHit::DismissStyleSuggestions)
    );
    // The label and the gaps focus the composer, like the rest of the band.
    assert_eq!(
        panel.hit_test(rect, centre(layout.lead)),
        Some(AIChatHit::FocusInput)
    );
}

#[test]
fn clicks_toggle_the_pick_and_dismiss_the_row() {
    let mut state = typing(three());
    assert_eq!(
        apply_chat_hit(&mut state, AIChatHit::ToggleSuggestedStyle(2), 0),
        ChatClickStep::Dirty
    );
    assert_eq!(
        state.chat.chosen_style.as_ref().map(|s| s.name.as_str()),
        Some("Swiss")
    );
    assert_eq!(
        apply_chat_hit(&mut state, AIChatHit::ToggleSuggestedStyle(2), 0),
        ChatClickStep::Dirty
    );
    assert_eq!(state.chat.chosen_style, None);
    assert_eq!(
        apply_chat_hit(&mut state, AIChatHit::ToggleSuggestedStyle(9), 0),
        ChatClickStep::Clean,
        "a stale index changes nothing"
    );
    assert_eq!(
        apply_chat_hit(&mut state, AIChatHit::DismissStyleSuggestions, 0),
        ChatClickStep::Dirty
    );
    assert!(!state.chat.has_style_suggest_row());
    assert_eq!(
        apply_chat_hit(&mut state, AIChatHit::DismissStyleSuggestions, 0),
        ChatClickStep::Clean
    );
}

#[test]
fn hover_names_the_chip_and_raises_a_card_with_the_blurb() {
    let mut state = typing(three());
    let panel = AIChatPlaceholder::from_editor(&state);
    let rect = panel_rect();
    let layout = layout(&state);
    assert_eq!(
        panel.footer_hover_at(rect, centre(layout.chips[1])),
        Some(ChatFooterButton::StyleSuggestChip(1))
    );
    assert_eq!(
        panel.footer_hover_at(rect, centre(layout.dismiss)),
        Some(ChatFooterButton::StyleSuggestDismiss)
    );

    // Resting: no card.
    let resting = paint(&state);
    assert!(!has_text_containing(&resting, "Dashboards, product pages"));
    state.editor_ui.chat_footer_hover = Some(ChatFooterButton::StyleSuggestChip(1));
    let hovered = paint(&state);
    let painted: String = hovered
        .texts
        .iter()
        .map(|(t, ..)| t.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        painted.contains("Dashboards,") && painted.contains("pages"),
        "{painted}"
    );
    assert!(painted.contains("next run"), "the choose tip: {painted}");
    // Chosen: the tip says how to let go.
    state.chat.toggle_chosen_style(1);
    let painted: String = paint(&state)
        .texts
        .iter()
        .map(|(t, ..)| t.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(painted.contains("again"), "the unchoose tip: {painted}");
    // The dismiss has its own tip.
    state.editor_ui.chat_footer_hover = Some(ChatFooterButton::StyleSuggestDismiss);
    let painted: String = paint(&state)
        .texts
        .iter()
        .map(|(t, ..)| t.clone())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(painted.contains("Hide"), "{painted}");
}

fn has_text_containing(backend: &PanelPaintBackend, needle: &str) -> bool {
    backend.texts.iter().any(|(t, ..)| t.contains(needle))
}

#[test]
fn the_row_is_hidden_while_running_minimized_composer_only_or_a_style_is_in_force() {
    let row_showing =
        |state: &EditorState| AIChatPlaceholder::from_editor(state).style_suggest_visible();
    assert!(row_showing(&typing(three())), "control");

    let mut running = typing(three());
    running.chat.messages[1].streaming = true;
    assert!(!row_showing(&running));
    let mut queued = typing(three());
    queued.chat.pending_send = Some("next".into());
    assert!(!row_showing(&queued));

    let mut minimized = typing(three());
    minimized.chat.minimize();
    assert!(!row_showing(&minimized));

    let state = typing(three());
    let mut composer = AIChatPlaceholder::from_editor(&state);
    composer.composer_only = true;
    assert!(!composer.style_suggest_visible());
    assert_eq!(composer.style_suggest_row_h(), 0.0);

    // A pinned style (its receipt chip is up) outranks a suggestion.
    let mut pinned = typing(three());
    pinned.editor_ui.pinned_style_guide = Some("dark-bold-mobile".into());
    let panel = AIChatPlaceholder::from_editor(&pinned);
    assert_eq!(panel.style_suggest_visible(), panel.style_receipt.is_none());
}

#[test]
fn the_row_coexists_with_the_selection_chip_line() {
    let mut state = typing(three());
    let screen: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame", "id": "home", "name": "Home", "width": 390, "height": 844
    }))
    .unwrap();
    state.active_children_mut().push(screen);
    state.set_single_selection(op_editor_core::NodeId::new("home"));
    let panel = AIChatPlaceholder::from_editor(&state);
    assert!((panel.chip_row_h() - 2.0 * CHIP_ROW_HEIGHT).abs() < 1e-3);
    let rows = panel.input_rows_rect(panel_rect());
    let line = panel.chip_row(rows).selection.expect("selection chip");
    let layout = layout(&state);
    assert!(
        layout.chips[0].origin.y >= line.origin.y + line.size.y,
        "the suggestions sit below the chip line"
    );
    // The selection chip's ✕ still resolves on its own line.
    let clear = panel.selection_chip_clear_rect(rows).expect("clear target");
    assert_eq!(
        panel.hit_test(panel_rect(), centre(clear)),
        Some(AIChatHit::ClearSelection)
    );
}

#[test]
fn geometry_without_the_row_is_unchanged() {
    // A panel with no suggestions lays out exactly as one that never had the
    // feature: same body, same composer, no extra band.
    let plain = typing(Vec::new());
    let mut cleared = typing(three());
    cleared.chat.clear_style_suggestions();
    let (a, b) = (
        AIChatPlaceholder::from_editor(&plain),
        AIChatPlaceholder::from_editor(&cleared),
    );
    let rect = panel_rect();
    assert!(rect_close(a.body_rect(rect), b.body_rect(rect)));
    assert!(rect_close(a.input_rect(rect), b.input_rect(rect)));
    assert!(rect_close(a.input_text_rect(rect), b.input_text_rect(rect)));
}

#[test]
fn a_pick_that_does_not_fit_still_shows_in_the_last_slot() {
    // Three long names: only two chips fit this row, but the picked third one
    // must stay on screen and keep resolving to its own index.
    let mut state = typing(three());
    state.chat.toggle_chosen_style(2);
    let layout = layout(&state);
    assert!(layout.chips.len() < 3, "{layout:?}");
    assert_eq!(layout.indices.last(), Some(&2), "{layout:?}");
    let last = *layout.chips.last().expect("a chip");
    let panel = AIChatPlaceholder::from_editor(&state);
    assert_eq!(
        panel.hit_test(panel_rect(), centre(last)),
        Some(AIChatHit::ToggleSuggestedStyle(2))
    );
    assert!(has_text_containing(&paint(&state), "Swiss"));
}
