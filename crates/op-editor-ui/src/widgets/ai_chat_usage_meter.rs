//! The chat panel's usage meter: what the last agent run really spent.
//!
//! One muted line under the transcript, e.g. `48.2k in · 85% cached · 3.1k
//! out`, built from the token counts the provider reported (`UsageView`, kept
//! on the chat tab). It is a read-out, not a price: there is no money amount,
//! because prices change and this repo holds no verified price table.
//!
//! It sits in the gap the panel already leaves between the transcript body and
//! the input block (`PAD + 8` tall), so showing or hiding it moves nothing: no
//! row is added to the input block and no transcript height changes. It is
//! hidden while a turn runs (the numbers would describe the previous run), in
//! the compact bar and in the composer-only card.
//!
//! Hovering the line raises a card with the exact numbers and one sentence on
//! why cache hits matter. The hover rides the footer hover slot
//! (`ChatFooterButton::UsageMeter`) so every host's existing clear paths cover
//! it; the line takes no clicks and shows no pointer cursor.

use op_editor_core::{format_token_count, ChatFooterButton, UsageView};

use crate::theme::Theme;
use crate::widgets::ai_chat_panel::{AIChatPlaceholder, PAD};
use crate::widgets::ai_chat_style_card::{draw_line, wrap_lines};
use crate::widgets::tooltip::TOOLTIP_RADIUS;
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect, TextLayout};

/// Height of the meter's row.
pub(crate) const METER_ROW_H: f32 = 16.0;
/// Space kept between the row and the input block below it. With
/// [`METER_ROW_H`] this fills all but 4 px of the 24 px gap above the input.
const METER_BOTTOM_GAP: f32 = 4.0;
const METER_FONT: f32 = 11.0;

const CARD_MAX_W: f32 = 280.0;
const CARD_MIN_W: f32 = 160.0;
const CARD_PAD: f32 = 8.0;
const CARD_FONT: f32 = 10.0;
const CARD_LINE_H: f32 = 14.0;
const CARD_PARAGRAPH_GAP: f32 = 4.0;
const CARD_PANEL_INSET: f32 = 4.0;
/// Space between the card's bottom edge and the row.
const CARD_ROW_GAP: f32 = 6.0;
const DETAIL_MAX_LINES: usize = 5;
const NOTE_MAX_LINES: usize = 3;

/// The meter's one line for `usage`. The cached share is left out when the run
/// reported no cache data at all (see `UsageView::cached_percent`).
pub(crate) fn usage_line(locale: op_editor_core::Locale, usage: &UsageView) -> String {
    let input = format_token_count(usage.total_input());
    let output = format_token_count(usage.output);
    match usage.cached_percent() {
        Some(percent) => op_i18n::translate(locale, "ai.usage.line")
            .replace("{input}", &input)
            .replace("{cached}", &percent.to_string())
            .replace("{output}", &output),
        None => op_i18n::translate(locale, "ai.usage.lineNoCache")
            .replace("{input}", &input)
            .replace("{output}", &output),
    }
}

/// The hover card's two paragraphs: the exact numbers, then the cache note.
pub(crate) fn usage_card_text(
    locale: op_editor_core::Locale,
    usage: &UsageView,
) -> (String, String) {
    let detail = op_i18n::translate(locale, "ai.usage.detail")
        .replace("{total}", &usage.total_input().to_string())
        .replace("{fresh}", &usage.input.to_string())
        .replace("{read}", &usage.cache_read.to_string())
        .replace("{write}", &usage.cache_write.to_string())
        .replace("{output}", &usage.output.to_string())
        .replace("{turns}", &usage.turns.to_string());
    let note = op_i18n::translate(locale, "ai.usage.cacheNote").to_string();
    (detail, note)
}

impl AIChatPlaceholder<'_> {
    /// The usage to show, or `None` when the meter must not be on screen: no
    /// run has reported usage, a turn is running or queued, or the panel is in
    /// a form with no transcript (compact bar, composer-only card).
    pub(crate) fn usage_meter(&self) -> Option<&UsageView> {
        if self.composer_only || self.state.is_minimized() || self.state.has_streaming_turn() {
            return None;
        }
        self.state.last_run_usage.as_ref()
    }

    /// The meter's row inside the panel at `rect`, directly above the input
    /// block. `None` while the meter is hidden. The single rect source paint
    /// and hover read.
    pub(crate) fn usage_meter_row(&self, rect: Rect) -> Option<Rect> {
        self.usage_meter()?;
        let input = self.input_rect(rect);
        Some(Rect::xywh(
            rect.origin.x + PAD,
            input.origin.y - METER_BOTTOM_GAP - METER_ROW_H,
            (rect.size.x - PAD * 2.0).max(0.0),
            METER_ROW_H,
        ))
    }

    /// Whether the cursor is over the meter's row.
    pub(crate) fn usage_meter_hover_at(
        &self,
        rect: Rect,
        point: Point2D,
    ) -> Option<ChatFooterButton> {
        self.usage_meter_row(rect)
            .filter(|row| row.contains(point))
            .map(|_| ChatFooterButton::UsageMeter)
    }

    /// Paint the meter line, and its hover card while the cursor is on it.
    /// Called near the end of the panel paint so the card hangs over the
    /// transcript.
    pub(crate) fn paint_usage_meter(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let (Some(usage), Some(row)) = (self.usage_meter(), self.usage_meter_row(rect)) else {
            return;
        };
        let hovered = self.footer_hover == Some(ChatFooterButton::UsageMeter);
        let line = usage_line(self.locale, usage);
        let text = text_metrics::fit_chrome(cx.backend, &line, row.size.x, METER_FONT);
        let color = if hovered {
            self.theme.foreground
        } else {
            self.theme.muted_foreground
        };
        let layout = TextLayout::single_run(
            &text,
            text_metrics::CHROME_FONT_FAMILY,
            METER_FONT,
            color.to_jian(),
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &layout,
            Point2D::new(
                row.origin.x,
                jian_widgets::centered_text_baseline_y(row, METER_FONT),
            ),
        );
        if hovered {
            let (detail, note) = usage_card_text(self.locale, usage);
            paint_usage_card(cx, &self.theme, row, rect, &detail, &note);
        }
    }
}

/// The card's rect and wrapped lines, resolved against the row and the panel.
pub(crate) struct UsageCardLayout {
    pub(crate) rect: Rect,
    pub(crate) detail: Vec<String>,
    pub(crate) note: Vec<String>,
}

/// Place the card above `row`, left-aligned with it and inside `panel`. Wrapped
/// with the chrome font the lines are painted with, so the card is as wide as
/// its text needs and never wider than the panel allows.
pub(crate) fn layout_usage_card(
    cx: &mut PaintCx<'_>,
    row: Rect,
    panel: Rect,
    detail: &str,
    note: &str,
) -> UsageCardLayout {
    let width = (panel.size.x - CARD_PANEL_INSET * 2.0).clamp(CARD_MIN_W, CARD_MAX_W);
    let inner = (width - CARD_PAD * 2.0).max(0.0);
    let detail = wrap_lines(cx, detail, inner, CARD_FONT, DETAIL_MAX_LINES);
    let note = wrap_lines(cx, note, inner, CARD_FONT, NOTE_MAX_LINES);
    let gap = if detail.is_empty() || note.is_empty() {
        0.0
    } else {
        CARD_PARAGRAPH_GAP
    };
    let height = CARD_PAD * 2.0 + (detail.len() + note.len()) as f32 * CARD_LINE_H + gap;
    let left = row.origin.x.clamp(
        panel.origin.x + CARD_PANEL_INSET,
        (panel.origin.x + panel.size.x - CARD_PANEL_INSET - width)
            .max(panel.origin.x + CARD_PANEL_INSET),
    );
    // Above the row; never above the panel's own top edge.
    let top = (row.origin.y - CARD_ROW_GAP - height).max(panel.origin.y + CARD_PANEL_INSET);
    UsageCardLayout {
        rect: Rect::xywh(left, top, width, height),
        detail,
        note,
    }
}

/// The card's tone. Where the raised surface is the panel's own tone (the
/// light theme: white on white) the card would be nothing but its shadow, so
/// it takes the muted tone instead and still reads as a card.
fn card_fill(theme: &Theme) -> crate::Color {
    let raised = crate::widgets::ai_chat_panel_paint::raised_surface(theme);
    if raised == theme.card {
        theme.muted
    } else {
        raised
    }
}

/// The card itself: also used by the style-suggestion hover.
pub(crate) fn paint_usage_card(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    row: Rect,
    panel: Rect,
    detail: &str,
    note: &str,
) {
    let layout = layout_usage_card(cx, row, panel, detail, note);
    // Tone alone separates the card: no outline, and no shadow, whose far
    // layer reads as a thick rule under a card this close to the line.
    cx.backend
        .fill_round_rect(layout.rect, TOOLTIP_RADIUS, card_fill(theme));
    cx.backend.save();
    cx.backend.clip_rect(layout.rect);
    let x = layout.rect.origin.x + CARD_PAD;
    let mut y = layout.rect.origin.y + CARD_PAD;
    for line in &layout.detail {
        draw_line(
            cx,
            line,
            CARD_FONT,
            theme.popover_foreground,
            x,
            y,
            CARD_LINE_H,
        );
        y += CARD_LINE_H;
    }
    if !layout.detail.is_empty() {
        y += CARD_PARAGRAPH_GAP;
    }
    for line in &layout.note {
        draw_line(
            cx,
            line,
            CARD_FONT,
            theme.muted_foreground,
            x,
            y,
            CARD_LINE_H,
        );
        y += CARD_LINE_H;
    }
    cx.backend.restore();
}
