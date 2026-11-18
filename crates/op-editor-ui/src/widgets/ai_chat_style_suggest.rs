//! The style-suggestion row above the composer.
//!
//! While a design prompt is typed and no style is pinned, the desktop host
//! offers up to three matching styles (`chat.style_suggestions`). They sit on
//! one line directly above the text area, under the pinned-style / selection
//! chips: a muted label, one chip per style and a ✕ that hides the row for the
//! current text. Clicking a chip picks that style for the NEXT run only (the
//! chip takes the selected tone and the label turns into `Style: <name>`);
//! clicking it again lets it go.
//!
//! The row is part of the input block, so it adds [`CHIP_ROW_HEIGHT`] only
//! while it shows and nothing at all otherwise: with no suggestions the
//! geometry of the panel is exactly what it was. It is hidden while a turn
//! runs, in the compact bar and in the composer-only card, and whenever a style
//! receipt is on screen (a pinned style or a design.md outranks a suggestion).
//!
//! Where each piece sits is resolved by [`layout_row`] only; paint, press and
//! hover all call it through [`AIChatPlaceholder::style_suggest_layout`]. Hover
//! rides the footer hover slot (`ChatFooterButton::StyleSuggestChip` /
//! `StyleSuggestDismiss`) so every host's existing clear paths cover it.

use op_editor_core::{ChatFooterButton, StyleSuggestion};

use crate::widgets::ai_chat_chip_row::{
    draw_chip_text, CHIP_FONT, CHIP_GAP, CHIP_H, CHIP_PAD_X, CHIP_RADIUS, CHIP_ROW_HEIGHT,
    CHIP_ROW_PAD_Y,
};
use crate::widgets::ai_chat_hit::AIChatHit;
use crate::widgets::ai_chat_panel::{footer_label_width, AIChatPlaceholder};
use crate::widgets::ai_chat_usage_meter::paint_usage_card;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::{text_metrics, PaintCx};
use crate::{Point2D, Rect};

/// Width of the trailing ✕ column (also its hit target).
const DISMISS_W: f32 = 18.0;
/// Narrowest a chip is squeezed to (a short name keeps its own, smaller, width):
/// below it a long style name stops being recognisable.
const CHIP_MIN_W: f32 = 96.0;
/// Widest a chip grows, so one long name cannot take the whole row.
const CHIP_MAX_W: f32 = 148.0;
/// Side of the palette glyph that stands in for the label on a narrow row.
const LEAD_ICON: f32 = 12.0;

/// Where each piece of the row sits.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SuggestRowLayout {
    /// The leading box: the label at its natural width, a palette glyph when
    /// the row is too narrow for the label, or zero-wide when it has no room.
    pub(crate) lead: Rect,
    /// Whether [`lead`] holds the glyph rather than the label.
    ///
    /// [`lead`]: SuggestRowLayout::lead
    pub(crate) lead_is_icon: bool,
    /// One rect per chip that fits, in order.
    pub(crate) chips: Vec<Rect>,
    /// Which entry of `chat.style_suggest_chips()` each rect stands for: the
    /// first `chips.len()` in order, except that a picked style that did not
    /// fit takes the last rect (a pick must never be off screen).
    pub(crate) indices: Vec<usize>,
    pub(crate) dismiss: Rect,
}

/// Place the lead, the chips and the ✕ on the row `row` (already
/// [`CHIP_ROW_HEIGHT`] tall). `label_w` and `chip_ws` are the widths each piece
/// would like (chips are capped at [`CHIP_MAX_W`]).
///
/// The row is narrow (the rail's panel leaves about 290 px), so what is kept
/// is decided in this order: the ✕; at least two chips (all of them when there
/// are fewer); then the label at its full width, never ellipsized (a cut label
/// reads as a bug), else a palette glyph in its place when that still leaves
/// two chips; then the rest of the chips. When everything does not fit, chips
/// shrink toward [`CHIP_MIN_W`] and only then are trailing chips dropped.
pub(crate) fn layout_row(row: Rect, label_w: f32, chip_ws: &[f32]) -> SuggestRowLayout {
    let top = row.origin.y + CHIP_ROW_PAD_Y;
    let dismiss = Rect::xywh(
        (row.origin.x + row.size.x - DISMISS_W).max(row.origin.x),
        top,
        DISMISS_W.min(row.size.x),
        CHIP_H,
    );
    let room = (dismiss.origin.x - row.origin.x - CHIP_GAP).max(0.0);
    let natural: Vec<f32> = chip_ws.iter().map(|w| w.min(CHIP_MAX_W)).collect();
    let minimum: Vec<f32> = natural.iter().map(|w| w.min(CHIP_MIN_W)).collect();
    let gaps = |n: usize| CHIP_GAP * n.saturating_sub(1) as f32;
    // How many chips fit in `avail` at their minimum widths.
    let fit = |avail: f32| {
        (0..=minimum.len())
            .rev()
            .find(|&n| minimum[..n].iter().sum::<f32>() + gaps(n) <= avail)
            .unwrap_or(0)
    };
    let wanted = natural.len().min(2);
    // (width of the lead, is it the glyph, chips that fit beside it)
    let lead_options = [(label_w, false), (LEAD_ICON, true)];
    let (lead_w, lead_is_icon, count, avail) = lead_options
        .iter()
        .filter(|(width, _)| *width > 0.0)
        .map(|&(width, icon)| (width, icon, room - width - CHIP_GAP))
        .map(|(width, icon, avail)| (width, icon, fit(avail), avail))
        .find(|&(_, _, count, _)| count >= wanted && count > 0)
        .unwrap_or((0.0, false, fit(room), room));

    // Start at the natural widths; take the excess from the chips that are
    // above their minimum, in proportion to how much they can give.
    let mut widths = natural[..count].to_vec();
    let excess = widths.iter().sum::<f32>() + gaps(count) - avail;
    if excess > 0.0 {
        let give: f32 = widths.iter().zip(&minimum).map(|(w, m)| w - m).sum();
        let share = if give > 0.0 {
            (excess / give).min(1.0)
        } else {
            0.0
        };
        for (width, min) in widths.iter_mut().zip(&minimum) {
            *width -= (*width - min) * share;
        }
    }
    let mut x = row.origin.x + if lead_w > 0.0 { lead_w + CHIP_GAP } else { 0.0 };
    let chips: Vec<Rect> = widths
        .iter()
        .map(|&width| {
            let chip = Rect::xywh(x, top, width, CHIP_H);
            x += width + CHIP_GAP;
            chip
        })
        .collect();
    SuggestRowLayout {
        lead: Rect::xywh(row.origin.x, top, lead_w, CHIP_H),
        lead_is_icon,
        indices: (0..chips.len()).collect(),
        chips,
        dismiss,
    }
}

impl AIChatPlaceholder<'_> {
    /// Whether the row is on screen at all.
    pub(crate) fn style_suggest_visible(&self) -> bool {
        !self.composer_only
            && !self.state.is_minimized()
            && !self.state.has_streaming_turn()
            && self.style_receipt.is_none()
            && self.state.has_style_suggest_row()
    }

    /// Height the row adds to the input block: `0` while hidden.
    pub(crate) fn style_suggest_row_h(&self) -> f32 {
        if self.style_suggest_visible() {
            CHIP_ROW_HEIGHT
        } else {
            0.0
        }
    }

    /// The row's label for the current state.
    fn style_suggest_label(&self) -> String {
        match &self.state.chosen_style {
            Some(chosen) => op_i18n::translate(self.locale, "ai.styleSuggest.chosen")
                .replace("{{name}}", &chosen.name),
            None => op_i18n::translate(self.locale, "ai.styleSuggest.label").to_string(),
        }
    }

    /// Natural width of a chip for `style`: its name between the label insets.
    fn style_chip_width(style: &StyleSuggestion) -> f32 {
        // The estimate is family-blind and runs a little short of the real glyphs.
        CHIP_PAD_X * 2.0 + footer_label_width(&style.name, CHIP_FONT) * 1.12
    }

    /// The row inside the input block with the notice already taken off
    /// (`rows_rect`): directly below the pinned-style / selection chip line.
    pub(crate) fn style_suggest_row(&self, rows_rect: Rect) -> Option<Rect> {
        if !self.style_suggest_visible() {
            return None;
        }
        let line_h = self.chip_row_h() - self.style_suggest_row_h();
        Some(Rect::xywh(
            rows_rect.origin.x,
            rows_rect.origin.y + line_h,
            rows_rect.size.x,
            CHIP_ROW_HEIGHT,
        ))
    }

    /// The single geometry source for paint, press and hover.
    pub(crate) fn style_suggest_layout(&self, rows_rect: Rect) -> Option<SuggestRowLayout> {
        let row = self.style_suggest_row(rows_rect)?;
        // The width estimate is family-blind: a few px of slack keep the label whole.
        let label_w = footer_label_width(&self.style_suggest_label(), CHIP_FONT) + 4.0;
        let chips = self.state.style_suggest_chips();
        let chip_ws: Vec<f32> = chips
            .iter()
            .map(|chip| Self::style_chip_width(chip))
            .collect();
        let mut layout = layout_row(row, label_w, &chip_ws);
        let picked = self
            .state
            .chosen_style
            .as_ref()
            .and_then(|chosen| chips.iter().position(|chip| *chip == chosen));
        if let (Some(picked), Some(last)) = (picked, layout.indices.last_mut()) {
            if picked >= layout.chips.len() {
                *last = picked;
            }
        }
        Some(layout)
    }

    /// What a press at `point` on the row means, if anything.
    pub(crate) fn style_suggest_press_at(
        &self,
        rows_rect: Rect,
        point: Point2D,
    ) -> Option<AIChatHit> {
        let layout = self.style_suggest_layout(rows_rect)?;
        if layout.dismiss.contains(point) {
            return Some(AIChatHit::DismissStyleSuggestions);
        }
        let slot = layout.chips.iter().position(|chip| chip.contains(point))?;
        Some(AIChatHit::ToggleSuggestedStyle(layout.indices[slot]))
    }

    /// Which piece of the row the cursor is over, for the hover wash and card.
    pub(crate) fn style_suggest_hover_at(
        &self,
        rect: Rect,
        point: Point2D,
    ) -> Option<ChatFooterButton> {
        let layout = self.style_suggest_layout(self.input_rows_rect(rect))?;
        if layout.dismiss.contains(point) {
            return Some(ChatFooterButton::StyleSuggestDismiss);
        }
        let slot = layout.chips.iter().position(|chip| chip.contains(point))?;
        u8::try_from(layout.indices[slot])
            .ok()
            .map(ChatFooterButton::StyleSuggestChip)
    }

    /// Paint the row (label, chips, ✕).
    pub(crate) fn paint_style_suggest_row(&self, cx: &mut PaintCx<'_>, rows_rect: Rect) {
        let Some(layout) = self.style_suggest_layout(rows_rect) else {
            return;
        };
        let theme = &self.theme;
        if layout.lead.size.x > 0.0 && layout.lead_is_icon {
            let icon = Point2D::new(
                layout.lead.origin.x,
                layout.lead.origin.y + (layout.lead.size.y - LEAD_ICON) / 2.0,
            );
            draw_icon(
                cx.backend,
                Icon::Palette,
                icon,
                LEAD_ICON,
                theme.muted_foreground,
                1.5,
            );
        } else if layout.lead.size.x > 0.0 {
            let label = self.style_suggest_label();
            let fitted =
                text_metrics::fit_chrome(cx.backend, &label, layout.lead.size.x, CHIP_FONT);
            draw_chip_text(cx, theme, &fitted, layout.lead.origin.x, layout.lead);
        }
        let chips = self.state.style_suggest_chips();
        for (chip, &index) in layout.chips.iter().zip(&layout.indices) {
            let style = chips[index];
            let chosen = self.state.chosen_style.as_ref() == Some(style);
            let hovered =
                self.footer_hover == Some(ChatFooterButton::StyleSuggestChip(index as u8));
            let fill = if chosen {
                theme.row_selected
            } else if hovered {
                theme.button_hover
            } else {
                theme.muted
            };
            cx.backend.fill_round_rect(*chip, CHIP_RADIUS, fill);
            let room = (chip.size.x - CHIP_PAD_X * 2.0).max(0.0);
            let fitted = text_metrics::fit_chrome(cx.backend, &style.name, room, CHIP_FONT);
            let x = text_metrics::centered_text_x(cx.backend, &fitted, CHIP_FONT, *chip);
            let color = if chosen {
                theme.foreground
            } else {
                theme.muted_foreground
            };
            draw_colored_chip_text(cx, &fitted, x, *chip, color);
        }
        if self.footer_hover == Some(ChatFooterButton::StyleSuggestDismiss) {
            cx.backend
                .fill_round_rect(layout.dismiss, CHIP_RADIUS, theme.button_hover);
        }
        let x = text_metrics::centered_text_x(cx.backend, "×", CHIP_FONT, layout.dismiss);
        draw_chip_text(cx, theme, "×", x, layout.dismiss);
    }

    /// The hover card of the chip (its blurb and what a click does) or of the
    /// ✕, hung above the row. Painted last so it covers the transcript.
    pub(crate) fn paint_style_suggest_hover(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let Some(layout) = self.style_suggest_layout(self.input_rows_rect(rect)) else {
            return;
        };
        let (anchor, detail, note) = match self.footer_hover {
            Some(ChatFooterButton::StyleSuggestChip(index)) => {
                let index = usize::from(index);
                let (Some(slot), Some(style)) = (
                    layout.indices.iter().position(|&shown| shown == index),
                    self.state.style_suggest_chips().get(index).copied(),
                ) else {
                    return;
                };
                let chip = &layout.chips[slot];
                let tip = if self.state.chosen_style.as_ref() == Some(style) {
                    "ai.styleSuggest.unchooseTip"
                } else {
                    "ai.styleSuggest.chooseTip"
                };
                let detail = if style.blurb.is_empty() {
                    style.name.clone()
                } else {
                    format!("{}: {}", style.name, style.blurb)
                };
                (
                    *chip,
                    detail,
                    op_i18n::translate(self.locale, tip).to_string(),
                )
            }
            Some(ChatFooterButton::StyleSuggestDismiss) => (
                layout.dismiss,
                op_i18n::translate(self.locale, "ai.styleSuggest.dismissTip").to_string(),
                String::new(),
            ),
            _ => return,
        };
        paint_usage_card(cx, &self.theme, anchor, rect, &detail, &note);
    }
}

/// [`draw_chip_text`] with an explicit colour (the chosen chip reads in the
/// foreground tone, the rest in the muted one).
fn draw_colored_chip_text(
    cx: &mut PaintCx<'_>,
    text: &str,
    x: f32,
    chip: Rect,
    color: crate::Color,
) {
    let layout = crate::TextLayout::single_run(
        text,
        text_metrics::CHROME_FONT_FAMILY,
        CHIP_FONT,
        color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &layout,
        Point2D::new(x, jian_widgets::centered_text_baseline_y(chip, CHIP_FONT)),
    );
}

#[cfg(test)]
#[path = "ai_chat_style_suggest_tests.rs"]
mod tests;
