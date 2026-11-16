//! Shared row-geometry + paint helpers for dropdown menus.
//!
//! `file_menu.rs` and `account_menu.rs` paint the same row chrome
//! (hover tint, hit rect, divider) with their own width/row-height
//! constants; the geometry-parameterized bodies live here once.

use crate::theme::Theme;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};

/// Full-width row hit rect at `(x, y)` for a `menu_width` × `row_height` row.
pub(crate) fn row_hit(x: f32, y: f32, point: Point2D, menu_width: f32, row_height: f32) -> bool {
    (Rect {
        origin: Point2D::new(x, y),
        size: Point2D::new(menu_width, row_height),
    })
    .contains(point)
}

/// Paint the per-row hover tint — a 4-px-inset round-rect washed with
/// `muted_foreground` at ~14% alpha (`#737373` light / `#9A9A9A` dark).
/// Earlier passes used `muted` (a near-invisible `#F5F5F5`-on-`#FFFFFF` in
/// light mode) then `button_hover` (a 6% overlay), but both were too faint
/// to read behind a dense icon+label row — only the empty right gutter
/// showed them, so the row looked un-hovered on the left. A 14% mid-grey
/// wash reads clearly across the WHOLE row on either theme.
pub(crate) fn paint_row_tint(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    x: f32,
    y: f32,
    menu_width: f32,
    row_height: f32,
) {
    let inset = 4.0;
    let mut tint = theme.muted_foreground;
    tint.a = 0.14;
    cx.backend.fill_round_rect(
        Rect {
            origin: Point2D::new(x + inset, y + 2.0),
            size: Point2D::new(menu_width - inset * 2.0, row_height - 4.0),
        },
        6.0,
        tint,
    );
}
