use crate::theme::Theme;
use crate::Color;
use op_editor_core::ThemeMode;

/// The Studio Home surface's colour tokens, expressed in the editor's
/// Figma UI3 vocabulary: a canvas-toned page, panel-toned cards, input-
/// toned fields and the `primary` accent. Surfaces are separated by
/// tone alone — there are deliberately no outline tokens, so a widget
/// cannot draw a hairline from this palette. Keeping them together makes
/// the light/dark contract testable without constructing a renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StudioPalette {
    /// Page background (the editor canvas tone).
    pub page: Color,
    /// Card / panel / top-bar surface.
    pub panel: Color,
    /// Primary ink.
    pub ink: Color,
    /// Muted ink.
    pub muted: Color,
    /// Welcome sub copy.
    pub sub: Color,
    /// Top-bar context copy.
    pub context: Color,
    /// The 1x17 brand divider.
    pub divider: Color,
    /// Primary accent (`theme.primary`).
    pub blue: Color,
    /// Primary hover.
    pub blue_hover: Color,
    /// Selected segment fill.
    pub blue_soft: Color,
    /// Segmented control track.
    pub segment_bg: Color,
    /// Input placeholder.
    pub placeholder: Color,
    /// Marker yellow `#F3FF23` (decoration only).
    pub yellow: Color,
    /// Disabled primary fill.
    pub disabled_primary: Color,
    /// Preview panel fill.
    pub preview: Color,
    /// Selected tab fill.
    pub selected_tab: Color,
    /// Tab hover.
    pub tab_hover: Color,
    /// Wide tab rest fill (transparent: quiet text tabs).
    pub tab_fill: Color,
    /// Wide tab hover fill.
    pub tab_hover_fill: Color,
    /// Wide selected tab fill.
    pub tab_selected_fill: Color,
    /// Outline-button hover fill.
    pub button_hover: Color,
    /// Preview eyebrow.
    pub eyebrow: Color,
    /// Preview description.
    pub preview_desc: Color,
    /// Preview footer.
    pub preview_footer: Color,
    /// Model status dot green.
    pub status_green: Color,
    /// Attachment chip fill.
    pub chip_bg: Color,
    /// Top bar fill.
    pub topbar: Color,
    /// A raised surface: secondary buttons, popovers and menus.
    pub raised: Color,
    /// An inset surface: the composer's own input box.
    pub surface_input: Color,
    /// Text accent. Same tone as `blue`; kept as its own token so link
    /// text can diverge from the button fill without touching call sites.
    pub link: Color,
    /// Selected task-tab label and its 2 px underline bar.
    pub tab_selected_ink: Color,
    pub tab_selected_bar: Color,
    /// Label on the disabled primary button.
    pub disabled_primary_ink: Color,
    /// The headline marker. Dark paints it as a thin underline instead of
    /// a band, because the band would sit behind white glyphs.
    pub marker: Color,
    /// Whether the marker paints as an underline below the baseline
    /// (dark) instead of a band across the glyphs' lower half (light).
    pub marker_underline: bool,
    /// Ink on the yellow sticker. Always dark — the sticker keeps its
    /// own colour, so the theme's light `ink` may not be used here.
    pub sticker_ink: Color,
    /// Paper backing under the transparent tutorial thumbnail, so its
    /// black artwork keeps a light sheet to sit on in dark mode.
    pub tutorial_sheet: Color,
    /// The normal-mode workspace canvas backdrop and its dot grid.
    pub canvas: Color,
    pub canvas_dot: Color,
    /// The "View example" pill on a tinted example card. It sits on the
    /// card's own colour, so it is a wash of white rather than a themed
    /// surface, with its own ink either way.
    pub card_link_fill: Color,
    pub card_link_ink: Color,
    /// Explore card tints (knowledge / tutorial / poster).
    pub tint_knowledge: Color,
    pub tint_tutorial: Color,
    pub tint_poster: Color,
    /// Ink of the glyph drawn on those tiles. The tiles keep their own
    /// bright colours in both themes, so the glyph — not the tile — is
    /// what has to flip to stay legible on lime.
    pub tile_ink: Color,
    /// The hover tooltip: a dark slab with light ink in both themes.
    pub tooltip_fill: Color,
    pub tooltip_ink: Color,
    /// Explore card icon tiles (knowledge / tutorial / poster).
    pub tile_knowledge: Color,
    pub tile_tutorial: Color,
    pub tile_poster: Color,
}

/// `color` at a fixed alpha (const twin of [`fade`]).
const fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

impl StudioPalette {
    /// The shared construction: every chrome token comes from the editor
    /// [`Theme`]; only the art tones (marker, tiles, tints, sticker) are
    /// hand-picked per mode.
    const fn from_theme(theme: Theme, dark: bool) -> Self {
        Self {
            page: theme.canvas_surface,
            panel: theme.card,
            ink: theme.foreground,
            muted: theme.muted_foreground,
            sub: theme.muted_foreground,
            context: theme.muted_foreground,
            divider: theme.row_selected,
            blue: theme.primary,
            blue_hover: Color::rgb_u8(0x0B, 0x84, 0xE0),
            blue_soft: theme.row_selected,
            segment_bg: theme.input,
            placeholder: with_alpha(theme.muted_foreground, 0.75),
            yellow: Color::rgb_u8(0xF3, 0xFF, 0x23),
            disabled_primary: with_alpha(theme.primary, 0.4),
            preview: theme.card,
            selected_tab: theme.card,
            tab_hover: theme.accent,
            tab_fill: with_alpha(theme.card, 0.0),
            tab_hover_fill: theme.accent,
            tab_selected_fill: theme.card,
            button_hover: theme.accent,
            eyebrow: theme.muted_foreground,
            preview_desc: theme.muted_foreground,
            preview_footer: theme.muted_foreground,
            status_green: theme.status_success,
            chip_bg: theme.muted,
            topbar: theme.card,
            raised: theme.muted,
            surface_input: theme.input,
            link: theme.primary,
            tab_selected_ink: theme.foreground,
            tab_selected_bar: theme.primary,
            disabled_primary_ink: Color::rgba_u8(0xFF, 0xFF, 0xFF, 0.6),
            marker: if dark {
                Color::rgba_u8(0xD5, 0xE8, 0x44, 0.85)
            } else {
                Color::rgb_u8(0xF3, 0xFF, 0x23)
            },
            marker_underline: dark,
            sticker_ink: if dark {
                Color::rgb_u8(0x18, 0x20, 0x19)
            } else {
                Color::rgb_u8(0x11, 0x1A, 0x32)
            },
            tutorial_sheet: if dark {
                Color::rgb_u8(0xE6, 0xF1, 0xFF)
            } else {
                Color::rgb_u8(0xFF, 0xFF, 0xFF)
            },
            canvas: theme.canvas_surface,
            canvas_dot: if dark {
                Color::rgb_u8(0x38, 0x38, 0x38)
            } else {
                Color::rgb_u8(0xD1, 0xD1, 0xD1)
            },
            card_link_fill: if dark {
                Color::rgba_u8(0xFF, 0xFF, 0xFF, 0.08)
            } else {
                Color::rgba_u8(0xFF, 0xFF, 0xFF, 0.85)
            },
            card_link_ink: theme.foreground,
            // The three example tiles keep their own bright accents
            // with dark icons drawn on them; dimming them would make the
            // page read as a different design rather than the same one
            // after dark.
            tint_knowledge: if dark {
                Color::rgb_u8(0x3A, 0x2F, 0x2A)
            } else {
                Color::rgb_u8(0xFF, 0xF3, 0xE8)
            },
            tint_tutorial: if dark {
                Color::rgb_u8(0x28, 0x34, 0x44)
            } else {
                Color::rgb_u8(0xED, 0xF5, 0xFF)
            },
            tint_poster: if dark {
                Color::rgb_u8(0x30, 0x37, 0x26)
            } else {
                Color::rgb_u8(0xF4, 0xFA, 0xDD)
            },
            tile_ink: if dark {
                Color::rgb_u8(0x17, 0x21, 0x36)
            } else {
                Color::rgb_u8(0xFF, 0xFF, 0xFF)
            },
            tooltip_fill: Color::rgb_u8(0x1E, 0x1E, 0x1E),
            tooltip_ink: Color::rgb_u8(0xFF, 0xFF, 0xFF),
            tile_knowledge: Color::rgb_u8(0xFF, 0x94, 0x42),
            tile_tutorial: Color::rgb_u8(0x68, 0xA6, 0xFF),
            tile_poster: Color::rgb_u8(0xC9, 0xFA, 0x15),
        }
    }

    pub const fn light() -> Self {
        Self::from_theme(Theme::light(), false)
    }

    /// The dark theme: the same design after dark, not a second one. The
    /// artwork, the yellow marker and the tinted example cards keep their
    /// own identity; only the chrome follows the editor's dark tokens.
    pub const fn dark() -> Self {
        Self::from_theme(Theme::dark(), true)
    }

    /// Every colour token at `factor` of its alpha — the entrance
    /// crossfade the Home and workspace surfaces play. It lives here so
    /// a new token cannot be forgotten by one of the three surfaces
    /// that fade the palette (they each used to keep their own copy of
    /// this list, and a token missing from one of them is invisible
    /// until someone watches that surface fade in).
    pub fn faded(self, factor: f32) -> Self {
        Self {
            page: fade(self.page, factor),
            panel: fade(self.panel, factor),
            ink: fade(self.ink, factor),
            muted: fade(self.muted, factor),
            sub: fade(self.sub, factor),
            context: fade(self.context, factor),
            divider: fade(self.divider, factor),
            blue: fade(self.blue, factor),
            blue_hover: fade(self.blue_hover, factor),
            blue_soft: fade(self.blue_soft, factor),
            segment_bg: fade(self.segment_bg, factor),
            placeholder: fade(self.placeholder, factor),
            yellow: fade(self.yellow, factor),
            disabled_primary: fade(self.disabled_primary, factor),
            preview: fade(self.preview, factor),
            selected_tab: fade(self.selected_tab, factor),
            tab_hover: fade(self.tab_hover, factor),
            tab_fill: fade(self.tab_fill, factor),
            tab_hover_fill: fade(self.tab_hover_fill, factor),
            tab_selected_fill: fade(self.tab_selected_fill, factor),
            button_hover: fade(self.button_hover, factor),
            eyebrow: fade(self.eyebrow, factor),
            preview_desc: fade(self.preview_desc, factor),
            preview_footer: fade(self.preview_footer, factor),
            status_green: fade(self.status_green, factor),
            chip_bg: fade(self.chip_bg, factor),
            topbar: fade(self.topbar, factor),
            raised: fade(self.raised, factor),
            surface_input: fade(self.surface_input, factor),
            link: fade(self.link, factor),
            tab_selected_ink: fade(self.tab_selected_ink, factor),
            tab_selected_bar: fade(self.tab_selected_bar, factor),
            disabled_primary_ink: fade(self.disabled_primary_ink, factor),
            marker: fade(self.marker, factor),
            sticker_ink: fade(self.sticker_ink, factor),
            tutorial_sheet: fade(self.tutorial_sheet, factor),
            canvas: fade(self.canvas, factor),
            canvas_dot: fade(self.canvas_dot, factor),
            card_link_fill: fade(self.card_link_fill, factor),
            card_link_ink: fade(self.card_link_ink, factor),
            tint_knowledge: fade(self.tint_knowledge, factor),
            tint_tutorial: fade(self.tint_tutorial, factor),
            tint_poster: fade(self.tint_poster, factor),
            tile_ink: fade(self.tile_ink, factor),
            tooltip_fill: fade(self.tooltip_fill, factor),
            tooltip_ink: fade(self.tooltip_ink, factor),
            tile_knowledge: fade(self.tile_knowledge, factor),
            tile_tutorial: fade(self.tile_tutorial, factor),
            tile_poster: fade(self.tile_poster, factor),
            marker_underline: self.marker_underline,
        }
    }

    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }
}

/// Multiply a colour's alpha by `factor` (composes with baked alpha).
pub(crate) fn fade(color: Color, factor: f32) -> Color {
    Color {
        a: color.a * factor,
        ..color
    }
}
