use crate::widgets::icons::{draw_icon, paint_icon_font_node, Icon};
use crate::{Color, Point2D, Rect, RenderBackend, TextLayout};

#[derive(Default)]
struct CountingBackend {
    paths: usize,
    fills: usize,
}

impl RenderBackend for CountingBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, _: &TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {
        self.paths += 1;
    }
    fn fill_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: f32, _: Color) {
        self.fills += 1;
    }
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn paint_one(icon: Icon) -> CountingBackend {
    let mut b = CountingBackend::default();
    draw_icon(
        &mut b,
        icon,
        Point2D::new(0.0, 0.0),
        16.0,
        Color::WHITE,
        1.5,
    );
    b
}

#[test]
fn plus_renders_two_paths() {
    let b = paint_one(Icon::Plus);
    assert_eq!(b.paths, 2);
}

#[test]
fn minus_renders_one_path() {
    let b = paint_one(Icon::Minus);
    assert_eq!(b.paths, 1);
}

#[test]
fn sun_renders_disc_plus_eight_rays() {
    let b = paint_one(Icon::Sun);
    assert_eq!(b.paths, 9);
}

#[test]
fn every_variant_paints_at_least_one_primitive() {
    for icon in [
        Icon::Cursor,
        Icon::Square,
        Icon::SquareRoundCorner,
        Icon::ChevronDown,
        Icon::Type,
        Icon::Frame,
        Icon::Hand,
        Icon::Undo,
        Icon::Redo,
        Icon::Braces,
        Icon::BookOpen,
        Icon::Library,
        Icon::Plus,
        Icon::Minus,
        Icon::Search,
        Icon::Sun,
        Icon::Globe,
        Icon::Maximize,
        Icon::Hash,
        Icon::PanelLeft,
        Icon::FolderOpen,
        Icon::FolderPlus,
        Icon::GitBranch,
        Icon::History,
        Icon::FilePlus,
        Icon::GitFork,
        Icon::Sparkles,
        Icon::Wand2,
        Icon::Close,
        Icon::Pen,
        Icon::ChevronUp,
        Icon::MessageSquare,
        Icon::LayoutGrid,
        Icon::Rows3,
        Icon::Columns3,
        Icon::RotateCw,
        Icon::Diamond,
        Icon::Component,
        Icon::Unlink,
        Icon::Check,
        Icon::ArrowUpRight,
        Icon::Wrench,
        Icon::Layers,
    ] {
        let b = paint_one(icon);
        assert!(b.paths > 0, "{:?} drew nothing", icon);
    }
}

#[test]
fn first_party_icon_font_names_all_resolve() {
    for name in [
        "calendar",
        "check",
        "chevron-down",
        "chevron-left",
        "chevron-right",
        "clock",
        "map-pin",
        "more-vertical",
        "play",
        "search",
        "star",
        "x",
        "arrow-right",
        "check-circle",
        "alert-triangle",
        "alert-octagon",
        "sticky-note",
        "bar-chart-2",
        "bold",
        "italic",
        "underline",
        "shopping-cart",
        "shopping-bag",
        "message-circle",
        "rocket",
        "menu",
        "credit-card",
        "trending-up",
        "trending-down",
        "compass",
        "refresh-cw",
        "layout-dashboard",
        "users",
        "package",
        "zap",
        "sliders-horizontal",
        "activity",
        "loader",
        "focus",
        "chart-line",
        "settings-2",
        "wrench",
        "layers",
    ] {
        assert!(
            Icon::from_name(name).is_some(),
            "first-party iconFontName {:?} fell through to placeholder",
            name
        );
    }
}

#[test]
fn bundled_iconify_catalog_contains_core_collections() {
    use crate::widgets::icon_catalog::{lookup_icon, IconRenderStyle};
    // lucide + feather are embedded in the wasm/binary (the core set).
    assert_eq!(
        lookup_icon("lucide", "airplay").map(|i| i.style),
        Some(IconRenderStyle::Stroke)
    );
    assert_eq!(
        lookup_icon("feather", "airplay").map(|i| i.style),
        Some(IconRenderStyle::Stroke)
    );
}

#[test]
fn brand_logos_resolve_after_runtime_registration() {
    use crate::widgets::icon_catalog::{lookup_icon, set_brand_catalog, IconRenderStyle};
    // simple-icons brand logos are NOT embedded — they load at runtime
    // (desktop: include_str at startup; web: fetched from the daemon). This test
    // owns registering the real brands asset; the catalog is a set-once global,
    // so other tests observe the same brand data regardless of ordering.
    set_brand_catalog(include_str!("../../assets/iconify-catalog-brands.json"));
    assert_eq!(
        lookup_icon("simple-icons", "github").map(|i| i.style),
        Some(IconRenderStyle::Fill)
    );
}

#[test]
fn icon_font_node_paints_simple_icon_as_fill_path() {
    // simple-icons are not embedded; register the brands catalog first
    // (idempotent set-once — independent of test ordering).
    crate::widgets::icon_catalog::set_brand_catalog(include_str!(
        "../../assets/iconify-catalog-brands.json"
    ));
    let mut b = CountingBackend::default();
    paint_icon_font_node(
        &mut b,
        "simple-icons",
        "github",
        Rect::xywh(0.0, 0.0, 24.0, 24.0),
        Some(Color::WHITE),
    );
    assert_eq!(b.fills, 1);
    assert_eq!(b.paths, 0);
}

#[test]
fn icon_font_node_paints_builtin_lucide_when_catalog_has_no_exact_name() {
    use crate::widgets::icon_catalog::lookup_icon;

    assert!(lookup_icon("lucide", "favorite").is_none());
    assert_eq!(Icon::from_name("favorite"), Some(Icon::Star));

    let mut b = CountingBackend::default();
    paint_icon_font_node(
        &mut b,
        "lucide",
        "favorite",
        Rect::xywh(0.0, 0.0, 24.0, 24.0),
        Some(Color::WHITE),
    );

    assert!(b.paths > 0);
    assert_eq!(b.fills, 0);
}

#[test]
fn icon_font_node_unknown_name_paints_nothing() {
    use crate::widgets::icon_catalog::lookup_icon;

    let unknown = "definitely-not-a-real-icon";
    assert!(lookup_icon("lucide", unknown).is_none());
    assert!(Icon::from_name(unknown).is_none());

    let mut lucide = CountingBackend::default();
    paint_icon_font_node(
        &mut lucide,
        "lucide",
        unknown,
        Rect::xywh(0.0, 0.0, 24.0, 24.0),
        Some(Color::WHITE),
    );
    assert_eq!(lucide.paths, 0);
    assert_eq!(lucide.fills, 0);

    let mut unknown_family = CountingBackend::default();
    paint_icon_font_node(
        &mut unknown_family,
        "missing-family",
        unknown,
        Rect::xywh(0.0, 0.0, 24.0, 24.0),
        Some(Color::WHITE),
    );
    assert_eq!(unknown_family.paths, 0);
    assert_eq!(unknown_family.fills, 0);
}

/// Invariant: an `icon_font` node resolves ANY glyph of the embedded lucide
/// and feather sets, not only the hand-copied chrome set behind
/// `Icon::from_name`. The painter asks the catalog first, so the ~75 chrome
/// glyphs are a fallback for legacy names, not a ceiling.
#[test]
fn icon_font_node_paints_every_lucide_and_feather_glyph() {
    use crate::widgets::icon_catalog::core_icon_names;

    let lucide: Vec<&str> = core_icon_names("lucide").collect();
    let feather: Vec<&str> = core_icon_names("feather").collect();
    assert!(lucide.len() >= 1_500, "lucide glyphs: {}", lucide.len());
    assert!(feather.len() >= 250, "feather glyphs: {}", feather.len());
    for (family, names) in [("lucide", &lucide), ("feather", &feather)] {
        for name in names {
            let mut backend = CountingBackend::default();
            paint_icon_font_node(
                &mut backend,
                family,
                name,
                Rect::xywh(0.0, 0.0, 24.0, 24.0),
                Some(Color::WHITE),
            );
            assert!(
                backend.paths + backend.fills > 0,
                "{family}:{name} painted nothing"
            );
        }
    }
}

/// Invariant: a name the chrome table does not know still paints through the
/// catalog (the case the first-party table could never cover), and a name both
/// know paints from the catalog, exactly as before.
#[test]
fn icon_font_node_names_outside_the_chrome_set_paint_through_the_catalog() {
    use crate::widgets::icon_catalog::lookup_icon;

    for name in [
        "binoculars",
        "croissant",
        "drum",
        "flower-2",
        "ghost",
        "guitar",
        "hand-coins",
        "microscope",
        "pizza",
        "telescope",
        "trophy",
        "wine",
    ] {
        assert!(lookup_icon("lucide", name).is_some(), "{name}");
        assert!(
            Icon::from_name(name).is_none(),
            "{name} is in the chrome table; pick a name that proves the catalog path"
        );
        let mut backend = CountingBackend::default();
        paint_icon_font_node(
            &mut backend,
            "lucide",
            name,
            Rect::xywh(0.0, 0.0, 24.0, 24.0),
            Some(Color::WHITE),
        );
        assert!(backend.paths > 0, "{name}");
    }
}

/// Invariant: the third-party notice for the embedded icon sets ships with
/// them. Lucide is ISC and Feather is MIT; both licences require the notice to
/// travel with copies, and the catalog asset holds their path data.
#[test]
fn embedded_icon_sets_have_their_licence_notice() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging/shared/THIRD_PARTY_NOTICES.md");
    let notices = std::fs::read_to_string(&path).expect("read THIRD_PARTY_NOTICES.md");
    for (heading, licence_marker) in [
        (
            "## Lucide (ISC)",
            "Permission to use, copy, modify, and/or distribute",
        ),
        (
            "## Feather (MIT)",
            "Permission is hereby granted, free of charge",
        ),
    ] {
        let start = notices
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} section is missing"));
        let section = &notices[start..];
        let end = section[heading.len()..]
            .find("\n## ")
            .map_or(section.len(), |at| at + heading.len());
        assert!(
            section[..end].contains(licence_marker),
            "{heading} lacks the licence text"
        );
    }
}
