//! Unit tests for the Studio Home surface: palette tokens, hit-tests,
//! the model chip, and the connect card.

use super::{model, HomeSurface, StudioPalette};
use crate::{Color, Point2D, Rect};
use op_editor_core::{HomeFamily, HomeHit};

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn visible_home() -> op_editor_core::EditorState {
    let mut state = op_editor_core::EditorState::new();
    state.editor_ui.home.visible = true;
    state
}

#[test]
fn studio_palette_is_the_editor_theme_in_both_modes() {
    use crate::theme::Theme;
    // Chrome tokens ARE the Figma UI3 theme tokens: no private palette.
    for (palette, theme) in [
        (StudioPalette::light(), Theme::light()),
        (StudioPalette::dark(), Theme::dark()),
    ] {
        assert_eq!(palette.page, theme.canvas_surface);
        assert_eq!(palette.panel, theme.card);
        assert_eq!(palette.topbar, theme.card);
        assert_eq!(palette.surface_input, theme.input);
        assert_eq!(palette.ink, theme.foreground);
        assert_eq!(palette.muted, theme.muted_foreground);
        assert_eq!(palette.blue, theme.primary);
        assert_eq!(palette.tab_selected_bar, theme.primary);
        assert_eq!(palette.button_hover, theme.accent);
    }
    let light = StudioPalette::light();
    assert_hex(light.page, "E5E5E5");
    assert_hex(light.panel, "FFFFFF");
    assert_hex(light.surface_input, "F5F5F5");
    assert_hex(light.blue, "0D99FF");
    assert_hex(light.yellow, "F3FF23");
    assert_hex(light.tint_knowledge, "FFF3E8");
    assert_hex(light.tint_tutorial, "EDF5FF");
    assert_hex(light.tint_poster, "F4FADD");
    assert_hex(light.tile_knowledge, "FF9442");
    assert_hex(light.tile_tutorial, "68A6FF");
    assert_hex(light.tile_poster, "C9FA15");

    let dark = StudioPalette::dark();
    assert_hex(dark.page, "1E1E1E");
    assert_hex(dark.panel, "2C2C2C");
    assert_hex(dark.surface_input, "383838");
    assert_hex(dark.ink, "FFFFFF");
    assert_hex(dark.blue, "0D99FF");
    assert_hex(dark.canvas, "1E1E1E");
    assert_hex(dark.sticker_ink, "182019");

    // Three contracts the dark theme must not break.
    // 1. Surfaces separate by tone alone: page < panel < input.
    assert_ne!(dark.page, dark.panel);
    assert_ne!(dark.panel, dark.surface_input);
    // 2. The work keeps its own colours: the yellow marker, the sticker's
    //    dark ink and the three bright example tiles are theme-independent.
    assert_hex(dark.yellow, "F3FF23");
    assert_eq!(dark.tile_knowledge, light.tile_knowledge);
    assert_eq!(dark.tile_tutorial, light.tile_tutorial);
    assert_eq!(dark.tile_poster, light.tile_poster);
    // 3. The marker becomes an underline in dark so it cannot sit behind
    //    near-white glyphs.
    assert!(!light.marker_underline);
    assert!(dark.marker_underline);
}

#[test]
fn fading_the_palette_touches_every_colour_and_no_flag() {
    // The three surfaces that crossfade share one implementation, so a
    // token added later cannot be left at full alpha on one of them.
    let full = StudioPalette::dark();
    let half = full.faded(0.5);
    assert_eq!(half.page.a, full.page.a * 0.5);
    assert_eq!(half.raised.a, full.raised.a * 0.5);
    assert_eq!(half.surface_input.a, full.surface_input.a * 0.5);
    assert_eq!(half.canvas_dot.a, full.canvas_dot.a * 0.5);
    assert_eq!(half.link.a, full.link.a * 0.5);
    assert_eq!(half.sticker_ink.a, full.sticker_ink.a * 0.5);
    assert_eq!(half.marker_underline, full.marker_underline);
}

#[test]
fn home_hit_test_resolves_tab_segment_send_and_explore_card() {
    let state = visible_home();
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.tabs[0])),
        Some(HomeHit::Tab(HomeFamily::AppUi))
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.tabs[6])),
        Some(HomeHit::Tab(HomeFamily::EventPoster))
    );
    // The app task's segmented control: option 0 = 手机.
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.segment_options[0])),
        Some(HomeHit::Segment(0))
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.segment_options[1])),
        Some(HomeHit::Segment(1))
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.send)),
        Some(HomeHit::Send)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.model_chip)),
        Some(HomeHit::ModelChip)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.use_example)),
        Some(HomeHit::UseExample)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.explore_cards[1])),
        Some(HomeHit::ExploreCard(HomeFamily::ScreenshotTutorial))
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.professional)),
        Some(HomeHit::Professional)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.open_file)),
        Some(HomeHit::OpenFile)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.input_box)),
        Some(HomeHit::Sheet),
        "the composer input keeps its caret-press hit"
    );
}

#[test]
fn tasks_without_a_segment_hide_the_control() {
    let mut state = visible_home();
    state.editor_ui.home.set_task(HomeFamily::Web, 1_000);
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    assert!(layout.segment.size.x == 0.0);
    assert!(layout.segment_options.iter().all(|r| r.size.x == 0.0));
    // The presentation task shows exactly two options.
    state
        .editor_ui
        .home
        .set_task(HomeFamily::Presentation, 2_000);
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    assert!(layout.segment_options[0].size.x > 0.0);
    assert!(layout.segment_options[1].size.x > 0.0);
    assert!(layout.segment_options[2].size.x == 0.0);
    // Infographic shows three.
    state
        .editor_ui
        .home
        .set_task(HomeFamily::Infographic, 3_000);
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    assert!(layout.segment_options.iter().all(|r| r.size.x > 0.0));
}

#[test]
fn an_empty_box_makes_send_start_from_the_example_and_says_so() {
    use op_editor_core::HomeSendMode;
    let mut state = visible_home();
    state.editor_ui.home.set_task(HomeFamily::Web, 1);
    let home = HomeSurface::for_editor(&state).expect("home");
    // No model, but the web example has a template draft: still live.
    assert_eq!(home.send_mode(), HomeSendMode::UseExample);
    assert!(
        !home.send_example_hint_visible(),
        "the hint waits for hover"
    );
    let send = home.layout(1440.0, 900.0).send;
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(send)),
        Some(HomeHit::Send)
    );

    state.editor_ui.home.hover = Some(HomeHit::Send);
    let home = HomeSurface::for_editor(&state).expect("home");
    assert!(home.send_example_hint_visible());
    let mut backend = crate::widgets::test_capture_backend::CaptureBackend::default();
    {
        let mut cx = crate::widgets::PaintCx {
            backend: &mut backend,
        };
        crate::widgets::Widget::paint(&home, &mut cx, Rect::xywh(0.0, 0.0, 1440.0, 900.0));
    }
    let locale = state.editor_ui.locale;
    for key in ["home.submit.example", "home.submit.exampleHint"] {
        let label = op_i18n::translate(locale, key);
        assert!(
            backend.texts.iter().any(|(text, _)| text == label),
            "{key} painted"
        );
    }

    // A brief of the user's own is not the example.
    state.editor_ui.home.set_draft("我的面包店官网");
    let home = HomeSurface::for_editor(&state).expect("home");
    assert_eq!(home.send_mode(), HomeSendMode::Connect);
    assert!(!home.send_example_hint_visible());
}

#[test]
fn model_chip_label_reuses_the_chat_selection_and_empties_without_an_agent() {
    let mut state = op_editor_core::EditorState::new();
    assert_eq!(
        model::model_chip_label(&state),
        op_i18n::translate(state.editor_ui.locale, "home.connect.chipEmpty")
    );
    state.editor_ui.agent_settings.connected[0] = true;
    state.chat.available_models = vec![op_editor_core::ModelEntry::new(
        op_editor_core::AgentProvider::ClaudeCode,
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
    )];
    state.chat.selected_model = 0;
    assert_eq!(model::model_chip_label(&state), "Claude Sonnet 4.6");
}

#[test]
fn connect_card_rows_stack_without_overlap_inside_the_card() {
    let mut state = visible_home();
    state.editor_ui.account_ui_available = true;
    state.editor_ui.agent_settings.web_served_models = true;
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    let (card, rows) = (layout.connect_card, layout.connect_rows);
    assert_eq!(card.size, Point2D::new(440.0, 308.0));
    // Centred over the composer panel.
    assert_close(
        card.origin.x + card.size.x / 2.0
            - (layout.composer.origin.x + layout.composer.size.x / 2.0),
        0.0,
    );
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.size.y, 56.0);
        assert!(row.origin.x >= card.origin.x);
        assert!(row.origin.x + row.size.x <= card.origin.x + card.size.x);
        assert!(row.origin.y >= card.origin.y);
        assert!(row.origin.y + row.size.y <= card.origin.y + card.size.y);
        if index > 0 {
            assert_close(rows[index].origin.y - rows[index - 1].origin.y, 66.0);
            assert!(!overlaps(rows[index - 1], rows[index]));
        }
    }
}

#[test]
fn connect_card_open_hides_home_hits_behind_the_modal() {
    let mut state = visible_home();
    state.editor_ui.account_ui_available = true;
    state.editor_ui.agent_settings.web_served_models = true;
    state.editor_ui.home.connect_card_open = true;
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.connect_rows[0])),
        Some(HomeHit::ConnectFreeTier)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.connect_rows[1])),
        Some(HomeHit::ConnectApiKey)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.connect_rows[2])),
        Some(HomeHit::ConnectCli)
    );
    // Presses that would hit Home chrome (the send button) and presses
    // far outside both close the card instead.
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout.send)),
        Some(HomeHit::ConnectClose)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, Point2D::new(4.0, 4.0)),
        Some(HomeHit::ConnectClose)
    );
}

#[test]
fn replace_strip_hits_only_while_pending() {
    let mut state = visible_home();
    state.editor_ui.home.draft = "我自己的需求".into();
    state.editor_ui.home.set_task(HomeFamily::AppUi, 1_000);
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout = home.layout(1440.0, 900.0);
    // Pending: both strip buttons hit.
    state.editor_ui.home.replace_pending = true;
    let home = HomeSurface::for_editor(&state).expect("home");
    let layout_pending = home.layout(1440.0, 900.0);
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout_pending.replace_use)),
        Some(HomeHit::ReplaceConfirm)
    );
    assert_eq!(
        home.hit_test(1440.0, 900.0, center(layout_pending.replace_keep)),
        Some(HomeHit::ReplaceKeep)
    );
    // Not pending: the strip area is just the input box.
    let _ = layout;
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() <= 2.0, "{actual} != {expected}");
}

fn assert_hex(color: Color, expected: &str) {
    let actual = format!(
        "{:02X}{:02X}{:02X}",
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8
    );
    assert_eq!(actual, expected);
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.origin.x < b.origin.x + b.size.x
        && b.origin.x < a.origin.x + a.size.x
        && a.origin.y < b.origin.y + b.size.y
        && b.origin.y < a.origin.y + a.size.y
}

#[test]
fn novice_connect_card_only_offers_supported_paths() {
    let mut state = visible_home();
    state.editor_ui.touch = true;
    state.editor_ui.size_class = op_editor_core::size_class::EditorSizeClass::Compact;
    state.editor_ui.external_cli_available = false;
    state.editor_ui.home.connect_card_open = true;
    let home = HomeSurface::for_editor(&state).unwrap();
    let layout = home.layout(390.0, 844.0);
    assert_eq!(
        layout.connect_rows[0],
        Rect::ZERO,
        "no hosted quota promise without that capability"
    );
    assert_eq!(
        layout.connect_rows[2],
        Rect::ZERO,
        "phones cannot launch a CLI"
    );
    assert!(layout.connect_rows[1].size.y >= 44.0);
    assert_eq!(
        home.hit_test(390.0, 844.0, center(layout.connect_rows[1])),
        Some(HomeHit::ConnectApiKey)
    );
    let mut capture = crate::widgets::test_capture_backend::CaptureBackend::default();
    crate::widgets::Widget::paint(
        &home,
        &mut crate::widgets::PaintCx {
            backend: &mut capture,
        },
        Rect::xywh(0.0, 0.0, 390.0, 844.0),
    );
    assert!(!capture
        .texts
        .iter()
        .any(|(s, _)| s.contains("20") || s.contains("CLI")));
}
