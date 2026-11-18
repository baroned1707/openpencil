//! The "Stock photo providers" block of the Images tab: the Pixabay and
//! Unsplash keys (masked like the Openverse client secret), a hint and a
//! "Get a free key" link under each, a Test button per key, and the opt-in
//! Unsplash switch with a link to the Unsplash API terms.
//!
//! One module owns the block's geometry, hit-testing and paint, so the three
//! walk the same rects. The block sits last in the tab, below the Image
//! Generation list, so the geometry of everything above it is unchanged;
//! `layout.rs` adds [`block_height_for_ui`] to the scroll height.

use super::paint::paint_search_test_status;
use super::*;
use crate::widgets::agent_settings_metrics::ROW_H_TWO_LINE;
use crate::widgets::agent_settings_rows::{paint_row_label, row_control_rect};
use crate::widgets::agent_settings_switch::{
    paint_settings_switch, SETTINGS_SWITCH_H, SETTINGS_SWITCH_W,
};
use crate::widgets::text_metrics;
use op_editor_core::agent_settings::StockProvider;

const DESKTOP_LABEL_W: f32 = 110.0;
const DESKTOP_HINT_H: f32 = 20.0;
const DESKTOP_ACTIONS_H: f32 = 36.0;
const DESKTOP_ROW_GAP: f32 = 6.0;
const DESKTOP_FIELD_H: f32 = 36.0;
const DESKTOP_LINK_W: f32 = 220.0;
const DESKTOP_TERMS_H: f32 = 28.0;
/// Wider than the Openverse Test button (56): this one is always visible, so
/// every locale's "Test" label must fit ("Проверить", "Tester").
const DESKTOP_TEST_W: f32 = 84.0;
const DESKTOP_TEST_H: f32 = 28.0;

const TOUCH_LABEL_H: f32 = 20.0;
const TOUCH_FIELD_H: f32 = 44.0;
const TOUCH_HINT_H: f32 = 28.0;
const TOUCH_ACTIONS_H: f32 = 44.0;
const TOUCH_ROW_GAP: f32 = 12.0;
const TOUCH_TOGGLE_H: f32 = 64.0;
const TOUCH_TERMS_H: f32 = 44.0;
const TOUCH_TEST_W: f32 = 92.0;

fn key_row_h(touch: bool) -> f32 {
    if touch {
        TOUCH_LABEL_H + TOUCH_FIELD_H + TOUCH_HINT_H + TOUCH_ACTIONS_H + TOUCH_ROW_GAP
    } else {
        DESKTOP_FIELD_H + DESKTOP_HINT_H + DESKTOP_ACTIONS_H + DESKTOP_ROW_GAP
    }
}

fn toggle_h(touch: bool) -> f32 {
    if touch {
        TOUCH_TOGGLE_H
    } else {
        ROW_H_TWO_LINE
    }
}

fn terms_h(touch: bool) -> f32 {
    if touch {
        TOUCH_TERMS_H
    } else {
        DESKTOP_TERMS_H
    }
}

/// Height of the whole block (title included).
pub(super) fn block_height_for_ui(content: Rect, touch: bool) -> f32 {
    let density = ImagesDensity::new(content, touch);
    density.section_title_h()
        + key_row_h(touch) * StockProvider::ALL.len() as f32
        + toggle_h(touch)
        + terms_h(touch)
}

fn row_index(provider: StockProvider) -> usize {
    match provider {
        StockProvider::Pixabay => 0,
        StockProvider::Unsplash => 1,
    }
}

/// The input field's focus value for `provider`.
pub(super) fn search_field(provider: StockProvider) -> ImageSearchField {
    match provider {
        StockProvider::Pixabay => ImageSearchField::PixabayKey,
        StockProvider::Unsplash => ImageSearchField::UnsplashKey,
    }
}

pub(super) fn provider_of_field(field: ImageSearchField) -> Option<StockProvider> {
    match field {
        ImageSearchField::PixabayKey => Some(StockProvider::Pixabay),
        ImageSearchField::UnsplashKey => Some(StockProvider::Unsplash),
        ImageSearchField::ClientId | ImageSearchField::ClientSecret => None,
    }
}

/// Every rect of one key row.
#[derive(Debug, Clone, Copy)]
pub(super) struct KeyRow {
    pub label: Rect,
    pub field: Rect,
    pub hint: Rect,
    pub link: Rect,
    pub test: Rect,
}

fn block_top(content: Rect, settings: &AgentSettings, touch: bool) -> f32 {
    let density = ImagesDensity::new(content, touch);
    super::layout::stock_block_top_for_ui(content, settings, density)
}

pub(super) fn key_row(
    content: Rect,
    settings: &AgentSettings,
    provider: StockProvider,
    touch: bool,
) -> KeyRow {
    let density = ImagesDensity::new(content, touch);
    let top = block_top(content, settings, touch)
        + density.section_title_h()
        + key_row_h(touch) * row_index(provider) as f32;
    let x = content.origin.x;
    let w = content.size.x;
    if touch {
        let label = Rect::xywh(x, top, w, TOUCH_LABEL_H);
        let field = Rect::xywh(x, top + TOUCH_LABEL_H, w, TOUCH_FIELD_H);
        let hint = Rect::xywh(x, field.origin.y + TOUCH_FIELD_H, w, TOUCH_HINT_H);
        let actions_y = hint.origin.y + TOUCH_HINT_H;
        let test = Rect::xywh(
            x + w - TOUCH_TEST_W,
            actions_y,
            TOUCH_TEST_W,
            TOUCH_ACTIONS_H,
        );
        let link = Rect::xywh(
            x,
            actions_y,
            (w - TOUCH_TEST_W - 12.0).clamp(44.0, DESKTOP_LINK_W),
            TOUCH_ACTIONS_H,
        );
        KeyRow {
            label,
            field,
            hint,
            link,
            test,
        }
    } else {
        let label = Rect::xywh(x, top, DESKTOP_LABEL_W, DESKTOP_FIELD_H);
        let field = Rect::xywh(
            x + DESKTOP_LABEL_W,
            top,
            w - DESKTOP_LABEL_W,
            DESKTOP_FIELD_H,
        );
        let hint = Rect::xywh(x, top + DESKTOP_FIELD_H, w, DESKTOP_HINT_H);
        let actions_y = hint.origin.y + DESKTOP_HINT_H;
        let link = Rect::xywh(x, actions_y, DESKTOP_LINK_W.min(w), DESKTOP_ACTIONS_H);
        let test = Rect::xywh(
            x + w - DESKTOP_TEST_W,
            actions_y + (DESKTOP_ACTIONS_H - DESKTOP_TEST_H) / 2.0,
            DESKTOP_TEST_W,
            DESKTOP_TEST_H,
        );
        KeyRow {
            label,
            field,
            hint,
            link,
            test,
        }
    }
}

pub(super) fn toggle_row(content: Rect, settings: &AgentSettings, touch: bool) -> Rect {
    let density = ImagesDensity::new(content, touch);
    let top = block_top(content, settings, touch)
        + density.section_title_h()
        + key_row_h(touch) * StockProvider::ALL.len() as f32;
    Rect::xywh(content.origin.x, top, content.size.x, toggle_h(touch))
}

pub(super) fn terms_link(content: Rect, settings: &AgentSettings, touch: bool) -> Rect {
    let row = toggle_row(content, settings, touch);
    Rect::xywh(
        content.origin.x,
        row.origin.y + row.size.y,
        DESKTOP_LINK_W.min(content.size.x),
        terms_h(touch),
    )
}

fn status_of(settings: &AgentSettings, provider: StockProvider) -> ImageTestStatus {
    match provider {
        StockProvider::Pixabay => settings.pixabay_test_status,
        StockProvider::Unsplash => settings.unsplash_test_status,
    }
}

fn key_is_blank(settings: &AgentSettings, provider: StockProvider) -> bool {
    match provider {
        StockProvider::Pixabay => settings.pixabay_api_key.is_blank(),
        StockProvider::Unsplash => settings.unsplash_access_key.is_blank(),
    }
}

/// A Test press needs a key and no probe already running.
fn test_enabled(settings: &AgentSettings, provider: StockProvider) -> bool {
    !key_is_blank(settings, provider) && status_of(settings, provider) != ImageTestStatus::Testing
}

pub(super) fn hit_test(
    content: Rect,
    settings: &AgentSettings,
    scrolled: Point2D,
    touch: bool,
) -> Option<ImagesHit> {
    for provider in StockProvider::ALL {
        let row = key_row(content, settings, provider, touch);
        if row.field.contains(scrolled) {
            return Some(ImagesHit::FocusSearchField(search_field(provider)));
        }
        if row.test.contains(scrolled) {
            return Some(if test_enabled(settings, provider) {
                ImagesHit::TestStockProvider(provider)
            } else {
                ImagesHit::None
            });
        }
        if row.link.contains(scrolled) {
            return Some(ImagesHit::OpenStockRegisterLink(provider));
        }
    }
    if toggle_row(content, settings, touch).contains(scrolled) {
        return Some(ImagesHit::ToggleUnsplash);
    }
    if terms_link(content, settings, touch).contains(scrolled) {
        return Some(ImagesHit::OpenUnsplashTerms);
    }
    None
}

/// Unscrolled rect of a focused stock key input.
pub(super) fn focused_field_rect(
    content: Rect,
    settings: &AgentSettings,
    field: ImageSearchField,
    touch: bool,
) -> Option<Rect> {
    let provider = provider_of_field(field)?;
    Some(key_row(content, settings, provider, touch).field)
}

pub(super) fn paint_stock(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    settings: &AgentSettings,
    ui: &EditorUiState,
    content: Rect,
    density: ImagesDensity,
    now_ms: u64,
) {
    let touch = density.touch;
    let top = block_top(content, settings, touch);
    let title_rect = Rect::xywh(
        content.origin.x,
        top,
        content.size.x,
        density.section_title_h(),
    );
    let title_font = if touch { 16.0 } else { 15.0 };
    let title = TextLayout::single_run(
        t_settings(ui, "settings.images.stockTitle"),
        "system-ui",
        title_font,
        theme.foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &title,
        Point2D::new(
            content.origin.x,
            if touch {
                jian_widgets::centered_text_baseline_y(title_rect, title_font)
            } else {
                top + 20.0
            },
        ),
    );

    for (provider, label_key, placeholder_key, hint_key) in [
        (
            StockProvider::Pixabay,
            "settings.images.pixabayKey",
            "settings.images.pixabayKeyPlaceholder",
            "settings.images.pixabayHint",
        ),
        (
            StockProvider::Unsplash,
            "settings.images.unsplashKey",
            "settings.images.unsplashKeyPlaceholder",
            "settings.images.unsplashHint",
        ),
    ] {
        let row = key_row(content, settings, provider, touch);
        paint_search_input_row(
            cx,
            theme,
            settings,
            ui,
            search_field(provider),
            t_settings(ui, label_key),
            t_settings(ui, placeholder_key),
            row.label,
            row.field,
            touch,
            now_ms,
        );
        paint_hint(cx, theme, row.hint, t_settings(ui, hint_key), touch);
        paint_link(
            cx,
            theme,
            row.link,
            t_settings(ui, "settings.images.getKeyLink"),
            touch,
        );
        paint_test_button(cx, theme, settings, ui, provider, row.test, touch);
    }

    let toggle = toggle_row(content, settings, touch);
    paint_row_label(
        cx,
        theme,
        toggle,
        t_settings(ui, "settings.images.unsplashToggle"),
        Some(t_settings(ui, "settings.images.unsplashToggleHint")),
        SETTINGS_SWITCH_W + 16.0,
    );
    paint_settings_switch(
        cx,
        theme,
        row_control_rect(toggle, SETTINGS_SWITCH_W, SETTINGS_SWITCH_H),
        settings.unsplash_enabled,
    );
    paint_link(
        cx,
        theme,
        terms_link(content, settings, touch),
        t_settings(ui, "settings.images.unsplashTerms"),
        touch,
    );
}

fn paint_hint(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect, text: &str, touch: bool) {
    let font = if touch { 13.0 } else { 12.0 };
    let text = ellipsize(cx, text, rect.size.x.max(44.0), font);
    let layout = TextLayout::single_run(
        &text,
        "system-ui",
        font,
        theme.muted_foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &layout,
        Point2D::new(
            rect.origin.x,
            if touch {
                jian_widgets::centered_text_baseline_y(rect, font)
            } else {
                rect.origin.y + 14.0
            },
        ),
    );
}

fn paint_link(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect, text: &str, touch: bool) {
    let font = if touch { 14.0 } else { 12.0 };
    let icon_size = if touch { 18.0 } else { 14.0 };
    let text = ellipsize(cx, text, (rect.size.x - icon_size - 10.0).max(44.0), font);
    let layout = TextLayout::single_run(
        &text,
        "system-ui",
        font,
        theme.primary.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &layout,
        Point2D::new(
            rect.origin.x,
            if touch {
                jian_widgets::centered_text_baseline_y(rect, font)
            } else {
                rect.origin.y + rect.size.y / 2.0 + 4.0
            },
        ),
    );
    let text_w = text_metrics::measure_chrome(cx.backend, &text, font);
    draw_icon(
        cx.backend,
        Icon::ArrowUpRight,
        Point2D::new(
            rect.origin.x + text_w + 6.0,
            rect.origin.y + (rect.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.primary,
        1.6,
    );
}

fn paint_test_button(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    settings: &AgentSettings,
    ui: &EditorUiState,
    provider: StockProvider,
    btn: Rect,
    touch: bool,
) {
    paint_search_test_status(cx, theme, status_of(settings, provider), btn, touch);
    let radius = if touch { 10.0 } else { 6.0 };
    cx.backend.fill_round_rect(btn, radius, theme.muted);
    let font = if touch { 15.0 } else { 13.0 };
    let text = t_settings(ui, "settings.images.test");
    let text_w = text_metrics::measure_chrome(cx.backend, text, font);
    let layout = TextLayout::single_run(
        text,
        "system-ui",
        font,
        if test_enabled(settings, provider) {
            theme.foreground
        } else {
            theme.muted_foreground
        }
        .to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &layout,
        Point2D::new(
            btn.origin.x + (btn.size.x - text_w) / 2.0,
            if touch {
                jian_widgets::centered_text_baseline_y(btn, font)
            } else {
                btn.origin.y + btn.size.y / 2.0 + 5.0
            },
        ),
    );
}
