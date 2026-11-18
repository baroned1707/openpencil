//! The quick switcher's popup (Cmd+K): a centred card below the top bar with
//! a query line and the matching rows.
//!
//! Flat and borderless like the rest of the chrome: the card separates from
//! the canvas by its raised tone and a soft shadow, the query sits in a well,
//! the highlighted row is a wash. What the rows ARE is
//! `op_editor_core::quick_switcher`; this file only places and paints them,
//! and `quick_switcher_flow` turns presses into state changes.
//!
//! Geometry is one pure function ([`QuickSwitcherLayout::new`]) that paint and
//! hit-testing both read, so a painted row and its click target cannot drift.

use op_editor_core::quick_switcher::{QuickSwitcherRow, VISIBLE_ROWS};
use op_editor_core::{Locale, QuickSwitcherState};

use crate::theme::Theme;
use crate::widgets::ai_chat_panel_paint::raised_surface;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::property_panel_text_input::paint_text_input_view;
use crate::widgets::text_metrics;
use crate::widgets::top_bar::TOP_BAR_HEIGHT;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

/// Width of the card on a roomy window.
pub const CARD_WIDTH: f32 = 520.0;
/// Space kept to the window's side edges, and between the top bar and the card.
const EDGE_MARGIN: f32 = 16.0;
const TOP_GAP: f32 = 12.0;
const PAD: f32 = 8.0;
pub const INPUT_HEIGHT: f32 = 36.0;
pub const ROW_HEIGHT: f32 = 32.0;
const EMPTY_HEIGHT: f32 = 40.0;
const RADIUS: f32 = 12.0;
const WELL_RADIUS: f32 = 8.0;
const SEARCH_GLYPH: f32 = 14.0;
const ROW_INSET: f32 = 12.0;
const FONT: f32 = 13.0;
const SMALL_FONT: f32 = 11.0;
const DIRTY_DOT: f32 = 5.0;
const OPEN_DOT: f32 = 4.0;
const GAP: f32 = 8.0;

/// The popup's strings, translated.
#[derive(Debug, Clone, Copy)]
pub struct QuickSwitcherText<'a> {
    pub placeholder: &'a str,
    pub empty: &'a str,
}

/// Where a press inside or outside the card landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickSwitcherHit {
    /// A result row, by its index in the full list (not the visible window).
    Row(usize),
    /// The card's chrome or its query line.
    Inside,
    Outside,
}

/// Where the card and its visible rows sit.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickSwitcherLayout {
    pub card: Rect,
    pub input: Rect,
    /// `(index in the full list, rect)` of each visible row, top to bottom.
    pub rows: Vec<(usize, Rect)>,
    /// No row matched: the card shows its "no matches" line instead.
    pub empty: bool,
}

impl QuickSwitcherLayout {
    /// Lay out the card for `row_count` rows with the list scrolled to
    /// `first_visible`, inside a `viewport_w` wide window.
    pub fn new(row_count: usize, first_visible: usize, viewport_w: f32) -> Self {
        let width = CARD_WIDTH.min((viewport_w - EDGE_MARGIN * 2.0).max(0.0));
        let x = (viewport_w - width) / 2.0;
        let y = TOP_BAR_HEIGHT + TOP_GAP;
        let first = first_visible.min(row_count.saturating_sub(VISIBLE_ROWS));
        let shown = row_count.saturating_sub(first).min(VISIBLE_ROWS);
        let body = if shown == 0 {
            EMPTY_HEIGHT
        } else {
            shown as f32 * ROW_HEIGHT
        };
        let height = PAD + INPUT_HEIGHT + PAD + body + PAD;
        let card = Rect::xywh(x, y, width, height);
        let input = Rect::xywh(x + PAD, y + PAD, (width - PAD * 2.0).max(0.0), INPUT_HEIGHT);
        let list_top = input.origin.y + input.size.y + PAD;
        let rows = (0..shown)
            .map(|offset| {
                (
                    first + offset,
                    Rect::xywh(
                        x + PAD,
                        list_top + offset as f32 * ROW_HEIGHT,
                        input.size.x,
                        ROW_HEIGHT,
                    ),
                )
            })
            .collect();
        Self {
            card,
            input,
            rows,
            empty: shown == 0,
        }
    }

    /// What `point` lands on.
    pub fn hit(&self, point: Point2D) -> QuickSwitcherHit {
        if !self.card.contains(point) {
            return QuickSwitcherHit::Outside;
        }
        self.rows
            .iter()
            .find(|(_, rect)| rect.contains(point))
            .map_or(QuickSwitcherHit::Inside, |(index, _)| {
                QuickSwitcherHit::Row(*index)
            })
    }
}

/// Paint the card: shadow, surface, query well, rows.
pub fn paint(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    layout: &QuickSwitcherLayout,
    state: &QuickSwitcherState,
    rows: &[QuickSwitcherRow],
    text: &QuickSwitcherText<'_>,
    now_ms: u64,
) {
    let card = layout.card;
    cx.backend.fill_drop_shadow(
        Rect::xywh(
            card.origin.x + 8.0,
            card.origin.y + 12.0,
            (card.size.x - 16.0).max(0.0),
            (card.size.y - 12.0).max(0.0),
        ),
        RADIUS,
        32.0,
        Color::rgb_u8(0, 0, 0).with_alpha(0.24),
    );
    cx.backend
        .fill_round_rect(card, RADIUS, raised_surface(theme));
    cx.backend.save();
    cx.backend.clip_rect(card);
    paint_query(cx, theme, layout.input, state, text, now_ms);
    if layout.empty {
        let band = Rect::xywh(
            card.origin.x,
            layout.input.origin.y + layout.input.size.y + PAD,
            card.size.x,
            EMPTY_HEIGHT,
        );
        let x = text_metrics::centered_text_x(cx.backend, text.empty, 12.0, band);
        cx.backend.draw_text(
            &TextLayout::single_run(
                text.empty,
                "system-ui",
                12.0,
                theme.muted_foreground.to_jian(),
                Point2D::new(0.0, 0.0),
            ),
            Point2D::new(x, jian_widgets::centered_text_baseline_y(band, 12.0)),
        );
    }
    for (index, rect) in &layout.rows {
        if let Some(row) = rows.get(*index) {
            paint_row(cx, theme, *rect, row, *index == state.selected);
        }
    }
    cx.backend.restore();
}

fn paint_query(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    well: Rect,
    state: &QuickSwitcherState,
    text: &QuickSwitcherText<'_>,
    now_ms: u64,
) {
    cx.backend.fill_round_rect(well, WELL_RADIUS, theme.input);
    draw_icon(
        cx.backend,
        Icon::Search,
        Point2D::new(
            well.origin.x + ROW_INSET - 2.0,
            well.origin.y + (well.size.y - SEARCH_GLYPH) / 2.0,
        ),
        SEARCH_GLYPH,
        theme.muted_foreground,
        1.5,
    );
    let text_x = well.origin.x + ROW_INSET - 2.0 + SEARCH_GLYPH + GAP;
    let field = Rect::xywh(
        text_x,
        well.origin.y,
        (well.origin.x + well.size.x - ROW_INSET - text_x).max(0.0),
        well.size.y,
    );
    paint_text_input_view(
        cx,
        theme,
        &state.input,
        field,
        FONT,
        0.0,
        jian_widgets::centered_text_baseline_y(well, FONT),
        now_ms,
        text.placeholder,
        true,
    );
}

fn paint_row(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    row: &QuickSwitcherRow,
    selected: bool,
) {
    if selected {
        cx.backend.fill_rect(rect, theme.row_selected_primary);
    }
    let baseline = jian_widgets::centered_text_baseline_y(rect, FONT);
    let right = rect.origin.x + rect.size.x - ROW_INSET;
    // The open mark: the Cmd+digit, or a dot for an open document no digit reaches.
    let mut right_limit = right;
    if row.open {
        right_limit = match row.digit {
            Some(digit) => {
                let mark = format!("\u{2318}{digit}");
                let width = text_metrics::measure_chrome(cx.backend, &mark, SMALL_FONT);
                draw(
                    cx,
                    &mark,
                    SMALL_FONT,
                    theme.muted_foreground,
                    Point2D::new(
                        right - width,
                        jian_widgets::centered_text_baseline_y(rect, SMALL_FONT),
                    ),
                );
                right - width
            }
            None => {
                cx.backend.fill_round_rect(
                    Rect::xywh(
                        right - OPEN_DOT,
                        rect.origin.y + (rect.size.y - OPEN_DOT) / 2.0,
                        OPEN_DOT,
                        OPEN_DOT,
                    ),
                    OPEN_DOT / 2.0,
                    theme.muted_foreground,
                );
                right - OPEN_DOT
            }
        } - GAP;
    }
    let left = rect.origin.x + ROW_INSET;
    // The project is muted and gives way first, then the name is ellipsized.
    let bullet = if row.dirty { GAP + DIRTY_DOT } else { 0.0 };
    let room = (right_limit - left - bullet).max(0.0);
    let project_width = if row.project.is_empty() {
        0.0
    } else {
        text_metrics::measure_chrome(cx.backend, &row.project, SMALL_FONT) + GAP
    };
    let label_full = text_metrics::measure_chrome(cx.backend, &row.label, FONT);
    let show_project = project_width > 0.0 && label_full + project_width <= room;
    let label = text_metrics::fit_chrome(cx.backend, &row.label, room, FONT);
    let label_width = text_metrics::measure_chrome(cx.backend, &label, FONT);
    draw(
        cx,
        &label,
        FONT,
        theme.foreground,
        Point2D::new(left, baseline),
    );
    let mut after = left + label_width;
    if row.dirty {
        cx.backend.fill_round_rect(
            Rect::xywh(
                after + GAP,
                rect.origin.y + (rect.size.y - DIRTY_DOT) / 2.0,
                DIRTY_DOT,
                DIRTY_DOT,
            ),
            DIRTY_DOT / 2.0,
            theme.foreground,
        );
        after += bullet;
    }
    if show_project {
        draw(
            cx,
            &row.project,
            SMALL_FONT,
            theme.muted_foreground,
            Point2D::new(
                after + GAP,
                jian_widgets::centered_text_baseline_y(rect, SMALL_FONT),
            ),
        );
    }
}

fn draw(cx: &mut PaintCx<'_>, text: &str, font: f32, color: Color, origin: Point2D) {
    if text.is_empty() {
        return;
    }
    cx.backend.draw_text(
        &TextLayout::single_run(text, "system-ui", font, color.to_jian(), Point2D::ZERO),
        origin,
    );
}

/// The strings for a locale (the flow resolves them once per frame).
pub fn text_for(locale: Locale) -> QuickSwitcherText<'static> {
    QuickSwitcherText {
        placeholder: op_i18n::translate(locale, "quickSwitcher.placeholder"),
        empty: op_i18n::translate(locale, "quickSwitcher.empty"),
    }
}

#[cfg(test)]
#[path = "quick_switcher_tests.rs"]
mod tests;
