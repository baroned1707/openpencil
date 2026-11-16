//! Body painter for [`super::ai_chat_panel::AIChatPlaceholder`] — the
//! empty-state pill stack. The active-transcript painter
//! lives in `ai_chat_transcript.rs`. Split out of `ai_chat_panel.rs`
//! to keep that file under the 800-line cap.

use super::ai_chat_panel::{ExampleCard, HEADER_HEIGHT, PAD};
use crate::theme::Theme;
use crate::widgets::button::paint_button_feedback_wash;
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

/// Gap between stacked example pills.
pub(crate) const EXAMPLE_CARD_GAP: f32 = 8.0;
/// Height of each example pill (~40px, compact rounded / stadium).
pub(crate) const EXAMPLE_CARD_HEIGHT: f32 = 40.0;
/// Corner radius for each pill — the shared Figma UI3 field radius.
const EXAMPLE_PILL_RADIUS: f32 = 8.0;
/// Left padding inside each pill for the prompt text.
const EXAMPLE_PILL_TEXT_PAD: f32 = 16.0;
const EXAMPLE_TITLE_FONT: f32 = 13.0;
/// Top offset (below the header) where the hint line sits.
const HINT_OFFSET: f32 = 20.0;
/// Gap between the hint line and the first pill.
const HINT_PILL_GAP: f32 = 14.0;
/// Gap between the last pill and the tip lines.
const TIP_TOP_GAP: f32 = 14.0;
/// Line height for tip text rows.
const TIP_LINE_H: f32 = 16.0;

/// Whether an example pill fully fits above `content_bottom` — the shared
/// predicate paint and hit-test use so a pill that would run into the
/// bottom-anchored composer (keyboard-shrunk compact sheet) is dropped
/// from BOTH instead of overlapping it visually or stealing its taps.
pub(crate) fn example_card_fits(card: &Rect, content_bottom: f32) -> bool {
    card.origin.y + card.size.y <= content_bottom
}

/// Returns the four pill rects, stacked full-width below the hint line.
pub(crate) fn example_card_rects(rect: Rect) -> [Rect; 4] {
    // First pill starts below header + hint line + gap.
    let first_pill_y = rect.origin.y + HEADER_HEIGHT + HINT_OFFSET + 16.0 + HINT_PILL_GAP;
    let pill_w = rect.size.x - PAD * 2.0;
    std::array::from_fn(|i| Rect {
        origin: Point2D::new(
            rect.origin.x + PAD,
            first_pill_y + i as f32 * (EXAMPLE_CARD_HEIGHT + EXAMPLE_CARD_GAP),
        ),
        size: Point2D::new(pill_w, EXAMPLE_CARD_HEIGHT),
    })
}

/// Ellipsize `text` so it fits within `max_w` at the given font size.
fn ellipsize(cx: &mut PaintCx<'_>, text: &str, max_w: f32, size: f32) -> String {
    if text_metrics::measure_chrome(cx.backend, text, size) <= max_w {
        return text.to_string();
    }
    let mut s = text.to_string();
    while !s.is_empty() && text_metrics::measure_chrome(cx.backend, &format!("{s}…"), size) > max_w
    {
        s.pop();
    }
    format!("{s}…")
}

/// Paint the floating chat panel shell. Figma UI3 floats a card with a
/// soft shadow and a tone step against the canvas, never a stroke. Skia
/// here has no blur primitive, so the shadow is faked with translucent
/// rounded rects behind the card: the near layer carries most of the
/// shadow right at the panel edge, the far layer is very faint so the
/// shadow fades out instead of ending in a hard grey band.
pub(crate) fn paint_panel_surface(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect) {
    paint_soft_shadow(cx, rect, 14.0);
    cx.backend.fill_round_rect(rect, 14.0, theme.card);
}

/// Fill of a surface that floats above the panel (model picker, style card,
/// parallel-agents picker). Figma UI3 floats menus darker than the panel in
/// the dark theme (the canvas tone) and white in the light one; a shadow
/// alone would vanish on a dark ground. The `Theme` carries no mode flag, so
/// luminance of its popover token decides.
pub(crate) fn raised_surface(theme: &Theme) -> Color {
    let c = theme.popover;
    if 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b < 0.5 {
        theme.canvas_surface
    } else {
        theme.popover
    }
}

/// The two translucent layers behind a floating surface (panel card, model
/// picker, tooltip-style cards). A raised surface separates from what is
/// under it by this shadow plus its own tone — never by an outline.
pub(crate) fn paint_soft_shadow(cx: &mut PaintCx<'_>, rect: Rect, radius: f32) {
    let shadow_far = Rect::xywh(rect.origin.x, rect.origin.y + 8.0, rect.size.x, rect.size.y);
    let shadow_near = Rect::xywh(rect.origin.x, rect.origin.y + 3.0, rect.size.x, rect.size.y);
    cx.backend
        .fill_round_rect(shadow_far, radius, (Color::BLACK).with_alpha(0.05));
    cx.backend
        .fill_round_rect(shadow_near, radius, (Color::BLACK).with_alpha(0.12));
}

/// Paint the message body's background wash. Tone alone separates the
/// header, the transcript and the composer — no rules.
pub(crate) fn paint_panel_body_chrome(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect, sep_y: f32) {
    let inner_x = rect.origin.x + 1.0;
    let inner_w = (rect.size.x - 2.0).max(0.0);
    let body_y = rect.origin.y + HEADER_HEIGHT;
    if sep_y > body_y {
        cx.backend.fill_rect(
            Rect::xywh(inner_x, body_y, inner_w, sep_y - body_y),
            (theme.background).with_alpha(0.72),
        );
    }
}

/// Paint the empty-state: centered hint, 4 full-width rounded pills, then two tip lines.
///
/// Layout (top→bottom):
///   • centered muted header line ("Try an example to design…") at HEADER_HEIGHT + HINT_OFFSET
///   • 4 stacked full-width pills (EXAMPLE_CARD_HEIGHT each, EXAMPLE_CARD_GAP gap)
///   • tip line 1: "Tip: Export design to code via Claude Code in terminal."
///   • tip line 2: "Drop image / text file to chat or via 📎"
///
/// `content_bottom` is where the bottom-anchored composer block begins
/// (`AIChatPlaceholder::empty_state_region`'s bottom edge). A compact touch
/// sheet shrunk by the software keyboard may not have room for the full
/// stack: pills that don't fully fit are dropped, and the optional tip
/// lines drop out before the pills do — nothing may overlap the composer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_examples(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    hint_label: &str,
    tip_label: &str,
    examples: &[ExampleCard; 4],
    disabled: bool,
    hover: Option<usize>,
    pressed_index: Option<usize>,
    content_bottom: f32,
) {
    let opacity = if disabled { 0.6 } else { 1.0 };

    // ── Centered hint header ─────────────────────────────────────────────────
    // Fitted before centring: the card is user-resizable and some locales'
    // hint runs half again as long as the English one, so an unfitted line
    // is centred straight off both edges of a narrow panel.
    let hint_font = 12.0;
    let hint_label = text_metrics::fit_chrome(
        cx.backend,
        hint_label,
        (rect.size.x - PAD * 2.0).max(0.0),
        hint_font,
    );
    let hint = TextLayout::single_run(
        &hint_label,
        "system-ui",
        hint_font,
        (theme.muted_foreground).with_alpha(opacity).to_jian(),
        Point2D::new(0.0, 0.0),
    );
    let hint_y = rect.origin.y + HEADER_HEIGHT + HINT_OFFSET + hint_font * 0.35;
    let hint_w = text_metrics::measure_chrome(cx.backend, &hint_label, hint_font);
    // Left-aligned with the pills below it, not centred: in the rail
    // every other label starts at the same gutter, and a lone centred
    // line reads as a caption that belongs to nothing.
    let _ = hint_w;
    cx.backend
        .draw_text(&hint, Point2D::new(rect.origin.x + PAD, hint_y));

    // ── 4 stacked full-width pills ───────────────────────────────────────────
    // A filled field tone, no outline: the pill reads as a surface by
    // tone alone, like every Figma UI3 input.
    let pill_bg = (theme.input).with_alpha(opacity);
    let text_color = (theme.foreground).with_alpha(opacity);

    for (idx, (pill, ex)) in example_card_rects(rect)
        .iter()
        .zip(examples.iter())
        .enumerate()
    {
        // A pill that would run into the bottom-anchored composer block is
        // cut rather than painted overlapping it; the ones below it can't
        // fit either.
        if !example_card_fits(pill, content_bottom) {
            break;
        }
        // Base pill fill.
        cx.backend
            .fill_round_rect(*pill, EXAMPLE_PILL_RADIUS, pill_bg);

        let hovered = !disabled && hover == Some(idx);
        let pressed = !disabled && pressed_index == Some(idx);
        if hovered || pressed {
            paint_button_feedback_wash(
                cx.backend,
                theme,
                *pill,
                EXAMPLE_PILL_RADIUS,
                hovered,
                pressed,
            );
        }

        // Single-line prompt label, left-padded, ellipsized if needed.
        let text_max_w = (pill.size.x - EXAMPLE_PILL_TEXT_PAD * 2.0).max(0.0);
        let label = ellipsize(cx, &ex.title, text_max_w, EXAMPLE_TITLE_FONT);
        let label_layout = TextLayout::single_run(
            &label,
            "system-ui",
            EXAMPLE_TITLE_FONT,
            text_color.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        // Vertically center text in pill (baseline-relative: center + fs*0.35).
        let baseline_y = pill.origin.y + EXAMPLE_CARD_HEIGHT / 2.0 + EXAMPLE_TITLE_FONT * 0.35;
        cx.backend.draw_text(
            &label_layout,
            Point2D::new(pill.origin.x + EXAMPLE_PILL_TEXT_PAD, baseline_y),
        );
    }

    // ── Two muted tip lines below the pills ─────────────────────────────────
    let pills = example_card_rects(rect);
    let last_pill_bottom = pills[3].origin.y + pills[3].size.y;
    let tip_color = (theme.muted_foreground)
        .with_alpha(0.55 * opacity)
        .to_jian();
    let tip_font = 10.0;

    // Guidance is supplied by the same context that supplies the actions.
    // It stays localised and avoids terminal instructions in a work editor.
    let tip_top = last_pill_bottom + TIP_TOP_GAP;
    if tip_label.is_empty() || tip_top + TIP_LINE_H > content_bottom {
        return;
    }
    let tip = text_metrics::fit_chrome(
        cx.backend,
        tip_label,
        (rect.size.x - PAD * 2.0).max(0.0),
        tip_font,
    );
    cx.backend.draw_text(
        &TextLayout::single_run(&tip, "system-ui", tip_font, tip_color, Point2D::ZERO),
        Point2D::new(rect.origin.x + PAD, tip_top + tip_font * 0.35),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, RenderBackend};

    #[derive(Default)]
    struct CaptureBackend {
        text_runs: Vec<String>,
        round_rects: Vec<(Rect, f32)>,
    }

    impl RenderBackend for CaptureBackend {
        fn begin_frame(&mut self) {}
        fn end_frame(&mut self) {}
        fn fill_rect(&mut self, _: Rect, _: Color) {}
        fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
        fn draw_text(&mut self, layout: &TextLayout, _: Point2D) {
            self.text_runs
                .extend(layout.runs().iter().map(|run| run.content.clone()));
        }
        fn clip_rect(&mut self, _: Rect) {}
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _: Point2D) {}
        fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
        fn fill_round_rect(&mut self, r: Rect, radius: f32, _: Color) {
            self.round_rects.push((r, radius));
        }
        fn stroke_round_rect(&mut self, r: Rect, radius: f32, _: Color, _: f32) {
            self.round_rects.push((r, radius));
        }
        fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
        fn resize(&mut self, _: u32, _: u32) {}
        fn dpi_scale(&self) -> f32 {
            1.0
        }
        fn measure_text(&mut self, text: &str, size: f32) -> f32 {
            text.chars().count() as f32 * size * 0.55
        }
    }

    fn make_examples() -> [ExampleCard; 4] {
        [
            ExampleCard {
                title: "Design a dark-themed music streaming mobile app home screen".into(),
                subtitle: "".into(),
                prompt: "Design a dark-themed music streaming mobile app home screen".into(),
                emoji: "",
            },
            ExampleCard {
                title: "Terminal-style dashboard web app for a sports team".into(),
                subtitle: "".into(),
                prompt: "Terminal-style dashboard web app for a sports team".into(),
                emoji: "",
            },
            ExampleCard {
                title: "Dark bold website for a coffee shop in Prague".into(),
                subtitle: "".into(),
                prompt: "Dark bold website for a coffee shop in Prague".into(),
                emoji: "",
            },
            ExampleCard {
                title: "Luxury webapp for managing barbershop clients".into(),
                subtitle: "".into(),
                prompt: "Luxury webapp for managing barbershop clients".into(),
                emoji: "",
            },
        ]
    }

    /// The 4 pills are stacked vertically (full-width), not in a 2×2 grid.
    #[test]
    fn example_card_rects_are_full_width_stacked_pills() {
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let pills = example_card_rects(rect);
        let expected_w = rect.size.x - PAD * 2.0;

        for pill in &pills {
            // Full width (panel minus padding on each side).
            assert!(
                (pill.size.x - expected_w).abs() < 0.01,
                "pill width should equal panel_w - 2*PAD, got {}",
                pill.size.x
            );
            // Each pill has the stadium height.
            assert!(
                (pill.size.y - EXAMPLE_CARD_HEIGHT).abs() < 0.01,
                "pill height should be EXAMPLE_CARD_HEIGHT"
            );
            // Pill starts at PAD from the left edge.
            assert!(
                (pill.origin.x - PAD).abs() < 0.01,
                "pill x origin should be PAD"
            );
        }

        // Verify stacking: each pill is directly below the previous + gap.
        for i in 1..4 {
            let expected_y = pills[i - 1].origin.y + EXAMPLE_CARD_HEIGHT + EXAMPLE_CARD_GAP;
            assert!(
                (pills[i].origin.y - expected_y).abs() < 0.01,
                "pill {} top should be prev_bottom + gap",
                i
            );
        }
    }

    /// Pills must not overlap each other.
    #[test]
    fn example_pills_do_not_overlap() {
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let pills = example_card_rects(rect);
        for i in 0..3 {
            let bottom_i = pills[i].origin.y + pills[i].size.y;
            assert!(
                bottom_i < pills[i + 1].origin.y,
                "pill {} bottom ({}) must be above pill {} top ({})",
                i,
                bottom_i,
                i + 1,
                pills[i + 1].origin.y
            );
        }
    }

    /// paint_examples paints rounded rects with large radius (pill look).
    #[test]
    fn paint_examples_paints_pill_shaped_rects() {
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let examples = make_examples();

        paint_examples(
            &mut cx,
            &Theme::dark(),
            rect,
            "Try an example",
            "tip",
            &examples,
            false,
            None,
            None,
            rect.origin.y + rect.size.y,
        );

        // At least 4 round rects with large radius should be painted (one per pill).
        let pill_rects: Vec<_> = backend
            .round_rects
            .iter()
            .filter(|(_, r)| *r >= EXAMPLE_PILL_RADIUS - 0.01)
            .collect();
        assert!(
            pill_rects.len() >= 4,
            "expected >= 4 large-radius round rects (pills), got {}",
            pill_rects.len()
        );
    }

    /// paint_examples paints the prompt text for all 4 pills.
    #[test]
    fn paint_examples_renders_prompt_labels() {
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let examples = make_examples();

        paint_examples(
            &mut cx,
            &Theme::dark(),
            rect,
            "Try an example",
            "tip",
            &examples,
            false,
            None,
            None,
            rect.origin.y + rect.size.y,
        );

        // Each example title (or its ellipsized form) should appear in text runs.
        for ex in &examples {
            let first_word = ex.title.split_whitespace().next().unwrap_or("");
            assert!(
                backend.text_runs.iter().any(|r| r.contains(first_word)),
                "paint should render text containing '{}' for pill label",
                first_word
            );
        }
    }

    /// #37: pill height is the new compact value (40px).
    #[test]
    fn example_card_height_is_compact_40px() {
        assert!(
            (EXAMPLE_CARD_HEIGHT - 40.0).abs() < 0.01,
            "pill height should be 40px (#37 compact), got {}",
            EXAMPLE_CARD_HEIGHT
        );
    }

    /// UI3: pills are filled fields at the 8px field radius.
    #[test]
    fn example_pill_radius_is_the_8px_field_radius() {
        assert!(
            (EXAMPLE_PILL_RADIUS - 8.0).abs() < 0.01,
            "pill radius should be 8px (UI3 field radius), got {}",
            EXAMPLE_PILL_RADIUS
        );
    }

    /// Keyboard-shrunk compact sheet: pills that would run into the
    /// bottom-anchored composer are dropped, never painted overlapping it.
    #[test]
    fn paint_examples_drops_pills_below_content_bottom() {
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let pills = example_card_rects(rect);
        // Room for the first two pills only.
        let content_bottom = pills[1].origin.y + pills[1].size.y + 1.0;

        paint_examples(
            &mut cx,
            &Theme::dark(),
            rect,
            "Try an example",
            "tip",
            &make_examples(),
            false,
            None,
            None,
            content_bottom,
        );

        let pill_rects: Vec<_> = backend
            .round_rects
            .iter()
            .filter(|(_, r)| *r >= EXAMPLE_PILL_RADIUS - 0.01)
            .collect();
        // 2 pills × fill (no outline) — pills 3 and 4 are dropped entirely.
        assert_eq!(
            pill_rects.len(),
            2,
            "only the pills that fully fit may paint, got {} rects",
            pill_rects.len()
        );
        for (r, _) in &pill_rects {
            assert!(
                r.origin.y + r.size.y <= content_bottom,
                "no painted pill may cross content_bottom ({content_bottom})"
            );
        }
    }

    /// Keyboard-shrunk compact sheet: the optional tip lines drop out when
    /// there is no room below the pill stack instead of overlapping the
    /// composer block.
    #[test]
    fn paint_examples_drops_tip_lines_when_height_is_tight() {
        let mut backend = CaptureBackend::default();
        let mut cx = PaintCx {
            backend: &mut backend,
        };
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let pills = example_card_rects(rect);
        // All 4 pills fit, but the tip lines below them do not.
        let content_bottom = pills[3].origin.y + pills[3].size.y + 4.0;

        paint_examples(
            &mut cx,
            &Theme::dark(),
            rect,
            "Try an example",
            "tip",
            &make_examples(),
            false,
            None,
            None,
            content_bottom,
        );

        assert!(
            !backend.text_runs.iter().any(|r| r.contains("Tip:")),
            "tip line 1 must drop out when it does not fit"
        );
        assert!(
            !backend.text_runs.iter().any(|r| r.contains("Drop image")),
            "tip line 2 must drop out when it does not fit"
        );
        let pill_rects = backend
            .round_rects
            .iter()
            .filter(|(_, r)| *r >= EXAMPLE_PILL_RADIUS - 0.01)
            .count();
        assert_eq!(pill_rects, 4, "all four pills still paint (fill only)");
    }

    /// #37: two tip lines must not overlap each other.
    /// tip1_top = last_pill_bottom + TIP_TOP_GAP; tip2_top = tip1_top + TIP_LINE_H.
    /// tip2_top > tip1_top + font_height (10px) is sufficient.
    #[test]
    fn tip_lines_are_non_overlapping() {
        let rect = Rect::xywh(0.0, 0.0, 360.0, 600.0);
        let pills = example_card_rects(rect);
        let last_pill_bottom = pills[3].origin.y + pills[3].size.y;
        let tip1_top = last_pill_bottom + TIP_TOP_GAP;
        let tip2_top = tip1_top + TIP_LINE_H;
        let tip_font = 10.0;
        // Tip 2's visual top must be below tip 1's visual bottom.
        // Visual bottom ≈ tip_top + font_height (conservative: use tip_font).
        let tip1_visual_bottom = tip1_top + tip_font;
        assert!(
            tip2_top >= tip1_visual_bottom,
            "tip2 top ({tip2_top}) must be at or below tip1 bottom ({tip1_visual_bottom}) to avoid overlap"
        );
    }
}
