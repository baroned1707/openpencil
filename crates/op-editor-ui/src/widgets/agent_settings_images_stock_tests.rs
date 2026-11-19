//! The Pixabay / Unsplash / Pexels rows of the Images tab: hit regions, geometry
//! against the rest of the tab, painted text (the key is masked) and the
//! press flow.

use crate::theme::Theme;
use crate::widgets::agent_settings_images::{self, hit_test_for_ui, paint_images_tab, ImagesHit};
use crate::widgets::agent_settings_panel::AgentSettingsHit;
use crate::widgets::agent_settings_press_flow::{
    apply_agent_settings_hit, SettingsPress, SettingsPressOutcome,
};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, RenderBackend, TextLayout};
use op_editor_core::agent_settings::{
    AgentSettings, ImageGenField, ImageSearchField, ImageTestStatus, SettingsFocus, StockProvider,
    PEXELS_DOCS_URL, PIXABAY_DOCS_URL, UNSPLASH_DOCS_URL, UNSPLASH_TERMS_URL,
};
use op_editor_core::host_settings_commit::SettingsCommitScope;
use op_editor_core::EditorState;

const PIXABAY_SECRET: &str = "px-very-secret-key-123";

#[derive(Default)]
struct CaptureBackend {
    texts: Vec<String>,
}

impl RenderBackend for CaptureBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, _: Point2D) {
        self.texts
            .extend(layout.runs().iter().map(|run| run.content.clone()));
    }
    fn clip_rect(&mut self, _: Rect) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn fill_oval(&mut self, _: Rect, _: Color) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text(&mut self, text: &str, size: f32) -> f32 {
        text.chars().count() as f32 * size * 0.5
    }
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn settings() -> AgentSettings {
    AgentSettings {
        pixabay_api_key: PIXABAY_SECRET.into(),
        ..AgentSettings::default()
    }
}

/// The geometry the hit test and paint both use, through the module's own
/// (private) accessors: probe by scanning rect centres on a fine grid is
/// avoided; instead walk the public hit test over a vertical sweep.
fn sweep(content: Rect, settings: &AgentSettings, touch: bool) -> Vec<(f32, ImagesHit)> {
    let height = agent_settings_images::responsive_content_height(settings, content.size.x, touch);
    let mut hits = Vec::new();
    let mut y = content.origin.y;
    // The controls sit at the left (links), middle (inputs) and right
    // (Test buttons, switches) of the column.
    let columns = [8.0, content.size.x / 2.0, content.size.x - 6.0];
    while y < content.origin.y + height {
        for column in columns {
            let at = Point2D::new(content.origin.x + column, y);
            hits.push((y, hit_test_for_ui(content, settings, at, touch)));
        }
        y += 2.0;
    }
    hits
}

#[test]
fn settings_images_rows_hit_test_and_paint() {
    let content = Rect::xywh(40.0, 80.0, 700.0, 0.0);
    let mut settings = settings();
    settings.add_image_gen_profile();

    for touch in [false, true] {
        let hits = sweep(content, &settings, touch);
        let seen = |wanted: ImagesHit| hits.iter().any(|(_, hit)| *hit == wanted);
        // Every control of the block is reachable and distinct.
        assert!(seen(ImagesHit::FocusSearchField(
            ImageSearchField::PixabayKey
        )));
        assert!(seen(ImagesHit::FocusSearchField(
            ImageSearchField::UnsplashKey
        )));
        assert!(seen(ImagesHit::FocusSearchField(
            ImageSearchField::PexelsKey
        )));
        assert!(seen(ImagesHit::OpenStockRegisterLink(
            StockProvider::Pexels
        )));
        assert!(seen(ImagesHit::TogglePexels));
        assert!(seen(ImagesHit::OpenStockRegisterLink(
            StockProvider::Pixabay
        )));
        assert!(seen(ImagesHit::OpenStockRegisterLink(
            StockProvider::Unsplash
        )));
        assert!(seen(ImagesHit::TestStockProvider(StockProvider::Pixabay)));
        assert!(seen(ImagesHit::ToggleUnsplash));
        assert!(seen(ImagesHit::OpenUnsplashTerms));
        // The Unsplash key is blank: its Test button is inert (no hit).
        assert!(!seen(ImagesHit::TestStockProvider(StockProvider::Unsplash)));
        assert!(!seen(ImagesHit::TestStockProvider(StockProvider::Pexels)));

        // The sweep passes the controls in reading order, after the
        // generation list, and never lands a stock hit inside the profile.
        let first_stock = hits
            .iter()
            .position(|(_, hit)| {
                matches!(
                    hit,
                    ImagesHit::FocusSearchField(ImageSearchField::PixabayKey)
                )
            })
            .unwrap();
        let last_profile = hits
            .iter()
            .rposition(|(_, hit)| {
                matches!(
                    hit,
                    ImagesHit::ToggleGenConfigEditor(_)
                        | ImagesHit::SetActiveGenConfig(_)
                        | ImagesHit::RemoveGenConfig(_)
                        | ImagesHit::FocusGenConfig { .. }
                )
            })
            .unwrap();
        assert!(
            last_profile < first_stock,
            "stock rows sit below the profiles"
        );
        let order = [
            ImagesHit::FocusSearchField(ImageSearchField::PixabayKey),
            ImagesHit::OpenStockRegisterLink(StockProvider::Pixabay),
            ImagesHit::FocusSearchField(ImageSearchField::UnsplashKey),
            ImagesHit::OpenStockRegisterLink(StockProvider::Unsplash),
            ImagesHit::FocusSearchField(ImageSearchField::PexelsKey),
            ImagesHit::OpenStockRegisterLink(StockProvider::Pexels),
            ImagesHit::ToggleUnsplash,
            ImagesHit::OpenUnsplashTerms,
            ImagesHit::TogglePexels,
        ];
        let positions: Vec<usize> = order
            .iter()
            .map(|wanted| hits.iter().position(|(_, hit)| hit == wanted).unwrap())
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{positions:?}"
        );

        // The scroll height holds the whole block (a hit exists near its end).
        let height = agent_settings_images::responsive_content_height(&settings, 700.0, touch);
        assert!(hits.last().unwrap().0 < content.origin.y + height);
    }

    // A running probe disables its own Test button.
    let mut testing = settings.clone();
    testing.pixabay_test_status = ImageTestStatus::Testing;
    assert!(!sweep(content, &testing, false)
        .iter()
        .any(|(_, hit)| *hit == ImagesHit::TestStockProvider(StockProvider::Pixabay)));

    // Paint: labels, hints, links and the Unsplash switch row; the stored
    // key shows as a mask, never as text.
    let mut state = EditorState::default();
    state.editor_ui.agent_settings = settings.clone();
    let theme = Theme::dark();
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint_images_tab(&mut cx, &theme, &settings, &state.editor_ui, content, 0);
    let painted = backend.texts.join(" | ");
    for expected in [
        "Stock photo providers",
        "Pixabay key",
        "Unsplash key",
        "Pexels key",
        "Use Unsplash",
        "Use Pexels",
        "Only the search keywords are sent to Pexels",
        "Get a free key",
        "Unsplash API terms",
        "Only the search keywords are sent to Pixabay",
        "Only the search keywords are sent to Unsplash",
        "Unsplash images are subject to Unsplash's API terms",
    ] {
        assert!(
            painted.contains(expected),
            "missing {expected:?} in {painted}"
        );
    }
    assert!(painted.contains("********"), "the stored key is masked");
    assert!(
        !painted.contains(PIXABAY_SECRET),
        "the key is never painted"
    );
}

#[test]
fn a_focused_stock_key_scrolls_to_its_own_input() {
    let content = Rect::xywh(0.0, 0.0, 700.0, 0.0);
    let settings = settings();
    for (field, provider) in [
        (ImageSearchField::PixabayKey, StockProvider::Pixabay),
        (ImageSearchField::UnsplashKey, StockProvider::Unsplash),
        (ImageSearchField::PexelsKey, StockProvider::Pexels),
    ] {
        let rect = agent_settings_images::focused_input_rect_for_ui(
            content,
            &settings,
            SettingsFocus::ImageSearch(field),
            false,
        )
        .expect("a stock key input has a rect even when Advanced is closed");
        assert_eq!(
            hit_test_for_ui(content, &settings, center(rect), false),
            ImagesHit::FocusSearchField(field),
            "{provider:?}"
        );
    }
    // Profile inputs are unaffected.
    assert!(agent_settings_images::focused_input_rect_for_ui(
        content,
        &settings,
        SettingsFocus::ImageGenProfile {
            index: 0,
            field: ImageGenField::Name
        },
        false
    )
    .is_none());
}

fn press(state: &mut EditorState, hit: AgentSettingsHit) -> SettingsPressOutcome {
    apply_agent_settings_hit(state, hit, SettingsCommitScope::Operator, 0)
}

#[test]
fn press_flow_runs_tests_toggles_and_opens_links() {
    let mut state = EditorState::default();
    // Unsplash is opt-in: a press flips it on, the next off.
    assert!(!state.editor_ui.agent_settings.unsplash_enabled);
    press(&mut state, AgentSettingsHit::ToggleUnsplash);
    assert!(state.editor_ui.agent_settings.unsplash_enabled);
    press(&mut state, AgentSettingsHit::ToggleUnsplash);
    assert!(!state.editor_ui.agent_settings.unsplash_enabled);

    // Pexels is opt-in too, and its switch is its own.
    assert!(!state.editor_ui.agent_settings.pexels_enabled);
    press(&mut state, AgentSettingsHit::TogglePexels);
    assert!(state.editor_ui.agent_settings.pexels_enabled);
    assert!(!state.editor_ui.agent_settings.unsplash_enabled);
    press(&mut state, AgentSettingsHit::TogglePexels);
    assert!(!state.editor_ui.agent_settings.pexels_enabled);

    // A Test press with no key reads Invalid at once and asks for no probe.
    press(
        &mut state,
        AgentSettingsHit::TestStockProvider(StockProvider::Pixabay),
    );
    let settings = &state.editor_ui.agent_settings;
    assert_eq!(settings.pixabay_test_status, ImageTestStatus::Invalid);
    assert_eq!(settings.pending_stock_provider_test, None);

    // With a key it raises the host seam and shows Testing; the other
    // provider's button is untouched.
    state.editor_ui.agent_settings.unsplash_access_key = "uk".into();
    press(
        &mut state,
        AgentSettingsHit::TestStockProvider(StockProvider::Unsplash),
    );
    let settings = &state.editor_ui.agent_settings;
    assert_eq!(settings.unsplash_test_status, ImageTestStatus::Testing);
    assert_eq!(
        settings.pending_stock_provider_test,
        Some(StockProvider::Unsplash)
    );
    assert_eq!(settings.pixabay_test_status, ImageTestStatus::Invalid);

    // The Pexels key behaves the same: blank is Invalid at once, a key raises
    // the host seam, and neither touches the other providers' buttons.
    press(
        &mut state,
        AgentSettingsHit::TestStockProvider(StockProvider::Pexels),
    );
    assert_eq!(
        state.editor_ui.agent_settings.pexels_test_status,
        ImageTestStatus::Invalid
    );
    state.editor_ui.agent_settings.pexels_api_key = "xk".into();
    press(
        &mut state,
        AgentSettingsHit::TestStockProvider(StockProvider::Pexels),
    );
    let settings = &state.editor_ui.agent_settings;
    assert_eq!(settings.pexels_test_status, ImageTestStatus::Testing);
    assert_eq!(
        settings.pending_stock_provider_test,
        Some(StockProvider::Pexels)
    );
    assert_eq!(settings.unsplash_test_status, ImageTestStatus::Testing);

    // Links open the registration pages and the Unsplash terms.
    for (hit, url) in [
        (
            AgentSettingsHit::OpenStockRegisterLink(StockProvider::Pixabay),
            PIXABAY_DOCS_URL,
        ),
        (
            AgentSettingsHit::OpenStockRegisterLink(StockProvider::Unsplash),
            UNSPLASH_DOCS_URL,
        ),
        (
            AgentSettingsHit::OpenStockRegisterLink(StockProvider::Pexels),
            PEXELS_DOCS_URL,
        ),
        (AgentSettingsHit::OpenUnsplashTerms, UNSPLASH_TERMS_URL),
    ] {
        assert_eq!(press(&mut state, hit).effect, SettingsPress::OpenUrl(url));
    }

    // Editing a key resets its Test result (the old verdict is about the old key).
    state.editor_ui.agent_settings.pixabay_test_status = ImageTestStatus::Valid;
    press(
        &mut state,
        AgentSettingsHit::FocusSearchField(ImageSearchField::PixabayKey),
    );
    state.editor_ui.settings_input.set_text("brand-new-key");
    op_editor_core::host_settings_commit::commit_settings_focus(
        &mut state,
        SettingsCommitScope::Operator,
        0,
    );
    let settings = &state.editor_ui.agent_settings;
    assert_eq!(settings.pixabay_api_key.expose(), "brand-new-key");
    assert_eq!(settings.pixabay_test_status, ImageTestStatus::Idle);

    // The same for the Pexels key.
    state.editor_ui.agent_settings.pexels_test_status = ImageTestStatus::Valid;
    press(
        &mut state,
        AgentSettingsHit::FocusSearchField(ImageSearchField::PexelsKey),
    );
    assert_eq!(state.editor_ui.settings_input.text(), "xk");
    state.editor_ui.settings_input.set_text("pexels-new-key");
    op_editor_core::host_settings_commit::commit_settings_focus(
        &mut state,
        SettingsCommitScope::Operator,
        0,
    );
    let settings = &state.editor_ui.agent_settings;
    assert_eq!(settings.pexels_api_key.expose(), "pexels-new-key");
    assert_eq!(settings.pexels_test_status, ImageTestStatus::Idle);
}
