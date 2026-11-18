//! The usage meter under the transcript: what paints, what hides it, that it
//! costs no layout, and what its hover card says.

use super::tests::PanelPaintBackend;
use super::tests_paint::rect_close;
use super::*;
use crate::widgets::ai_chat_usage_meter::{usage_card_text, usage_line, METER_ROW_H};
use op_editor_core::{ChatFooterButton, ChatMessage, Locale, UsageView};

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

fn finished_run(usage: Option<UsageView>) -> EditorState {
    let mut state = EditorState::new();
    // The expanded panel only exists on the rail's Agent tab.
    state.editor_ui.enter_chat_tab();
    state.chat.messages.push(ChatMessage::user("design a page"));
    state.chat.messages.push(ChatMessage::assistant("Done."));
    state.chat.last_run_usage = usage;
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

fn meter_texts(
    backend: &PanelPaintBackend,
) -> Vec<&(String, f32, jian_core::scene::Color, Point2D)> {
    backend
        .texts
        .iter()
        .filter(|(text, ..)| text.contains(" in \u{b7} ") || text.contains(" in · "))
        .collect()
}

#[test]
fn meter_paints_and_hides_correctly() {
    // With cache data: tokens in (all input), the cached share, tokens out.
    // 4,000 + 41,000 + 3,200 = 48,200 in; 41,000 / 48,200 = 85%.
    let backend = paint(&finished_run(Some(WITH_CACHE)));
    let line = "48.2k in · 85% cached · 3.1k out";
    let found: Vec<_> = backend
        .texts
        .iter()
        .filter(|(text, ..)| text == line)
        .collect();
    assert_eq!(found.len(), 1, "texts: {:?}", backend.texts);
    let (_, size, color, origin) = found[0];
    assert!((size - 11.0).abs() < 1e-4);
    let theme = Theme::dark();
    assert_eq!(
        *color,
        theme.muted_foreground.to_jian(),
        "the resting meter is muted"
    );
    // It sits in the gap above the input block, not over the transcript body
    // and not inside the composer.
    let state = finished_run(Some(WITH_CACHE));
    let panel = AIChatPlaceholder::from_editor(&state);
    let row = panel.usage_meter_row(panel_rect()).expect("meter row");
    let input = panel.input_rect(panel_rect());
    let body = panel.body_rect(panel_rect());
    assert!(row.origin.y >= body.origin.y + body.size.y);
    assert!(row.origin.y + row.size.y <= input.origin.y);
    assert!((row.size.y - METER_ROW_H).abs() < 1e-4);
    assert!(origin.y >= row.origin.y && origin.y <= row.origin.y + row.size.y + 4.0);

    // Without cache data: just in and out.
    let backend = paint(&finished_run(Some(NO_CACHE)));
    assert!(
        backend
            .texts
            .iter()
            .any(|(text, ..)| text == "48.2k in · 3.1k out"),
        "texts: {:?}",
        backend.texts
    );
    assert!(!backend
        .texts
        .iter()
        .any(|(text, ..)| text.contains("cached")));

    // No usage: no meter.
    assert!(meter_texts(&paint(&finished_run(None))).is_empty());

    // A running turn hides it: its numbers would describe the previous run.
    let mut running = finished_run(Some(WITH_CACHE));
    running.chat.messages[1].streaming = true;
    assert!(meter_texts(&paint(&running)).is_empty());
    let mut queued = finished_run(Some(WITH_CACHE));
    queued.chat.pending_send = Some("next".into());
    assert!(meter_texts(&paint(&queued)).is_empty());

    // The compact bar has no transcript to sit under.
    let mut minimized = finished_run(Some(WITH_CACHE));
    minimized.chat.minimize();
    assert!(meter_texts(&paint(&minimized)).is_empty());
    // Nor does the composer-only card.
    let state = finished_run(Some(WITH_CACHE));
    let mut composer = AIChatPlaceholder::from_editor(&state);
    composer.composer_only = true;
    assert!(composer.usage_meter().is_none());
    assert!(composer.usage_meter_row(panel_rect()).is_none());
}

#[test]
fn meter_adds_no_shape_and_no_layout() {
    // Square chrome: the resting meter is text only. Same fills, same round
    // rects, same input height and transcript body with and without it.
    let shown = finished_run(Some(WITH_CACHE));
    let hidden = finished_run(None);
    let with = paint(&shown);
    let without = paint(&hidden);
    assert_eq!(with.fills.len(), without.fills.len());
    assert_eq!(with.round_rects.len(), without.round_rects.len());
    assert_eq!(with.stroke_lines, without.stroke_lines);

    let shown_panel = AIChatPlaceholder::from_editor(&shown);
    let hidden_panel = AIChatPlaceholder::from_editor(&hidden);
    let rect = panel_rect();
    assert_eq!(
        shown_panel.input_height_for_rect(rect),
        hidden_panel.input_height_for_rect(rect)
    );
    assert!(rect_close(
        shown_panel.body_rect(rect),
        hidden_panel.body_rect(rect)
    ));
}

#[test]
fn meter_hover_raises_the_card_with_exact_numbers() {
    let mut state = finished_run(Some(WITH_CACHE));
    let panel = AIChatPlaceholder::from_editor(&state);
    let rect = panel_rect();
    let row = panel.usage_meter_row(rect).expect("meter row");
    let inside = Point2D::new(row.origin.x + 10.0, row.origin.y + row.size.y / 2.0);
    assert_eq!(
        panel.footer_hover_at(rect, inside),
        Some(ChatFooterButton::UsageMeter)
    );
    let above = Point2D::new(inside.x, row.origin.y - 2.0);
    assert_eq!(panel.footer_hover_at(rect, above), None);
    // Hidden meter: nothing to hover.
    let hidden = finished_run(None);
    assert_eq!(
        AIChatPlaceholder::from_editor(&hidden).footer_hover_at(rect, inside),
        None
    );

    // Hovered: the card paints above the row, inside the panel, and carries
    // every exact number plus the cache sentence (no price claim).
    state.editor_ui.chat_footer_hover = Some(ChatFooterButton::UsageMeter);
    let backend = paint(&state);
    let (detail, note) = usage_card_text(Locale::EnUs, &WITH_CACHE);
    assert!(detail.contains("48200") && detail.contains("41000") && detail.contains("3200"));
    assert!(detail.contains("4000") && detail.contains("3100") && detail.contains('6'));
    assert_eq!(note, "Cache hits cost a fraction of normal input tokens.");
    let painted: String = backend
        .texts
        .iter()
        .map(|(text, ..)| text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for word in note.split_whitespace() {
        assert!(
            painted.contains(word),
            "card is missing `{word}`: {painted}"
        );
    }
    assert!(painted.contains("48200") && painted.contains("41000"));
    let card = backend
        .round_rects
        .iter()
        .filter(|(r, radius, _)| {
            (radius - crate::widgets::tooltip::TOOLTIP_RADIUS).abs() < 1e-4
                && r.origin.y + r.size.y <= row.origin.y
                && r.size.x >= 160.0
        })
        .map(|(r, ..)| *r)
        .next_back()
        .expect("the hover card paints above the row");
    assert!(
        card.origin.x >= rect.origin.x
            && card.origin.x + card.size.x <= rect.origin.x + rect.size.x
    );
    assert!(card.origin.y >= rect.origin.y);
    // Resting: no card.
    state.editor_ui.chat_footer_hover = None;
    assert!(!paint(&state)
        .texts
        .iter()
        .any(|(text, ..)| text.contains("48200")));
}

#[test]
fn meter_takes_no_clicks() {
    let state = finished_run(Some(WITH_CACHE));
    let panel = AIChatPlaceholder::from_editor(&state);
    let rect = panel_rect();
    let row = panel.usage_meter_row(rect).expect("meter row");
    let point = Point2D::new(row.origin.x + 10.0, row.origin.y + row.size.y / 2.0);
    let hidden = finished_run(None);
    // Exactly what the same point resolves to when the meter is not there.
    assert_eq!(
        panel.hit_test(rect, point),
        AIChatPlaceholder::from_editor(&hidden).hit_test(rect, point)
    );
}

#[test]
fn every_locale_fills_the_meter_templates() {
    for locale in Locale::ALL {
        for usage in [WITH_CACHE, NO_CACHE] {
            let line = usage_line(locale, &usage);
            assert!(
                !line.contains('{') && !line.contains('}'),
                "{locale:?}: {line}"
            );
            assert!(
                line.contains("48.2k") && line.contains("3.1k"),
                "{locale:?}: {line}"
            );
            let (detail, note) = usage_card_text(locale, &usage);
            assert!(!detail.contains('{'), "{locale:?}: {detail}");
            assert!(!note.is_empty());
        }
        assert!(usage_line(locale, &WITH_CACHE).contains("85"), "{locale:?}");
    }
}
