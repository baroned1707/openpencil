//! Figma UI3 chrome shared by every floating menu, picker and dialog.
//!
//! The UI3 shell has no hairline strokes: a floating surface is a rounded
//! fill with a soft shadow, a hovered row is a flat tint, and groups are
//! separated by spacing (or, at most, a 1-px `theme.muted` fill rect).
//! These helpers hold that vocabulary once so the menus do not each respell
//! it. They live beside the File menu — the first surface restyled — and are
//! reached as `file_menu::popup::*`.

use crate::render_backend::RenderBackend;
use crate::theme::Theme;
use crate::{Color, Point2D, Rect};
use jian_widgets::Tokens;

/// Corner radius of a menu / popover surface.
pub(crate) const MENU_RADIUS: f32 = 10.0;
/// Corner radius jian's shared `Select` popup paints itself with (a literal
/// in the vendored widget, so a shadow under it must match).
pub(crate) const SELECT_RADIUS: f32 = 6.0;
/// Corner radius jian's shared `ColorPicker` panel paints itself with.
pub(crate) const COLOR_PICKER_RADIUS: f32 = 8.0;
/// Corner radius of a dialog / modal surface.
pub(crate) const DIALOG_RADIUS: f32 = 12.0;
/// Corner radius of filled dialog buttons and inputs.
pub(crate) const CONTROL_RADIUS: f32 = 6.0;

/// Whether `theme` is the dark palette (decides how heavy shadows must be
/// to stay visible against a dark canvas).
pub(crate) fn is_dark(theme: &Theme) -> bool {
    let c = theme.popover;
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b < 0.5
}

/// Soft drop shadow under `rect`: six stacked low-alpha black round-rects,
/// each a little larger and lower than the last. Dark surfaces get a heavier
/// shadow because the shadow is their only edge against a dark canvas.
pub(crate) fn paint_soft_shadow(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    rect: Rect,
    radius: f32,
) {
    let weight: f32 = if is_dark(theme) { 2.4 } else { 1.0 };
    // (grow, drop, alpha) — outermost layer first; many faint steps so the
    // falloff reads as a blur rather than as concentric outlines.
    for (grow, drop, alpha) in [
        (12.0, 8.0, 0.012),
        (9.0, 6.5, 0.016),
        (6.5, 5.0, 0.022),
        (4.0, 3.5, 0.030),
        (2.0, 2.5, 0.040),
        (0.5, 1.5, 0.050),
    ] {
        backend.fill_round_rect(
            Rect {
                origin: Point2D::new(rect.origin.x - grow, rect.origin.y - grow + drop),
                size: Point2D::new(rect.size.x + grow * 2.0, rect.size.y + grow * 2.0),
            },
            radius + grow,
            Color::rgb_u8(0, 0, 0).with_alpha((alpha * weight).min(0.5)),
        );
    }
}

/// A borderless floating surface: soft shadow, then the opaque popover fill.
pub(crate) fn paint_popup_surface(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    rect: Rect,
    radius: f32,
) {
    paint_soft_shadow(backend, theme, rect, radius);
    backend.fill_round_rect(rect, radius, theme.popover);
}

/// Hover wash for one menu row: `theme.accent`, inset 4 px from the surface
/// edge and 2 px top/bottom so adjacent rows never touch.
pub(crate) fn paint_row_hover(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    x: f32,
    y: f32,
    menu_width: f32,
    row_height: f32,
) {
    backend.fill_round_rect(
        Rect {
            origin: Point2D::new(x + 4.0, y + 2.0),
            size: Point2D::new(menu_width - 8.0, row_height - 4.0),
        },
        6.0,
        theme.accent,
    );
}

/// Group separator: a 1-px `theme.muted` fill rect inset by `pad_x`, with
/// `gap` above and below. Returns the y just past the block (the same
/// advance as the retired stroked divider, so hit-testing is unchanged).
pub(crate) fn paint_group_divider(
    backend: &mut dyn RenderBackend,
    theme: &Theme,
    rect: Rect,
    y: f32,
    pad_x: f32,
    gap: f32,
) -> f32 {
    backend.fill_rect(
        Rect {
            origin: Point2D::new(rect.origin.x + pad_x, y + gap),
            size: Point2D::new(rect.size.x - pad_x * 2.0, 1.0),
        },
        theme.muted,
    );
    y + gap * 2.0 + 1.0
}

/// Shared-control tokens for the jian widgets that paint themselves
/// (`Select`, `ColorPicker`): the `border` hairline is switched off and the
/// row hover wash becomes the flat `accent` tone.
pub(crate) fn borderless_tokens(theme: &Theme) -> Tokens {
    let mut tokens = crate::widgets::button::tokens_from_theme(theme);
    tokens.border = theme.border.with_alpha(0.0);
    tokens.button_hover = theme.accent;
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_capture_backend::CaptureBackend;

    #[test]
    fn surface_is_shadow_layers_then_one_opaque_popover_fill() {
        let theme = Theme::dark();
        let rect = Rect::xywh(100.0, 100.0, 200.0, 120.0);
        let mut backend = CaptureBackend::default();
        paint_popup_surface(&mut backend, &theme, rect, MENU_RADIUS);
        let (last_rect, _, last_color) = *backend.round_fills.last().expect("surface fill");
        assert_eq!(last_rect, rect);
        assert_eq!(last_color, theme.popover);
        assert!(
            backend.round_fills.len() > 1,
            "a soft shadow sits under the fill"
        );
        for (shadow_rect, _, color) in &backend.round_fills[..backend.round_fills.len() - 1] {
            assert!(color.a < 0.2, "shadow layers stay faint");
            assert!(shadow_rect.size.x > rect.size.x, "each layer grows outward");
        }
    }

    #[test]
    fn group_divider_is_a_one_pixel_muted_fill_that_keeps_the_old_advance() {
        let theme = Theme::light();
        let rect = Rect::xywh(0.0, 0.0, 300.0, 200.0);
        let mut backend = CaptureBackend::default();
        let next = paint_group_divider(&mut backend, &theme, rect, 40.0, 12.0, 4.0);
        assert_eq!(next, 40.0 + 4.0 * 2.0 + 1.0);
        assert_eq!(backend.rect_fills.len(), 1);
        assert_eq!(backend.rect_fills[0].0.size.y, 1.0);
        assert_eq!(backend.rect_fills[0].1, theme.muted);
    }
}
