use crate::theme::Theme;
use crate::widgets::button::tokens_from_theme;
use crate::widgets::PaintCx;
use crate::Rect;
use jian_widgets::components::switch::Switch;

pub(super) const SETTINGS_SWITCH_W: f32 = 36.0;
pub(super) const SETTINGS_SWITCH_H: f32 = 20.0;

/// Paint the settings switch via the canonical jian `Switch` (pill track,
/// white round knob), Figma UI3 style: `primary` track when on, the
/// `input` tone when off. `enabled` here is the switch's ON state — the
/// control is always interactive, so jian's `enabled` is fixed `true`.
pub(super) fn paint_settings_switch(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    enabled: bool,
) {
    paint_with_tokens(cx, theme, rect, enabled, false);
}

/// Same switch for a row that sits on a `muted` card, where the `input`
/// off-track would vanish into the card: the off track takes the inset
/// panel tone instead.
pub(super) fn paint_settings_switch_on_card(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    enabled: bool,
) {
    paint_with_tokens(cx, theme, rect, enabled, true);
}

fn paint_with_tokens(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    enabled: bool,
    on_card: bool,
) {
    let mut tokens = tokens_from_theme(theme);
    if on_card {
        tokens.input = theme.card;
    }
    Switch {
        on: enabled,
        enabled: true,
        hovered: false,
        pressed: false,
    }
    .paint(cx.backend, rect, &tokens);
}
