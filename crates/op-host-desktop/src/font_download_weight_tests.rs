//! A downloaded Google variable font must serve every weight the document
//! asks for. `jian-skia` used to instance a variable font once (wght 400), so a
//! bold request rendered as regular plus synthetic bold. These tests drive the
//! real resolver, the same one paint and measure use.
#![cfg(not(target_os = "windows"))]

use jian_skia::font_resolve::FontResolver;
use skia_safe::{Font, FontMgr, FourByteTag, Typeface};

const INTER_VF: &[u8] = include_bytes!("../assets/fonts/Inter-VF.ttf");
const MANROPE_VF: &[u8] = include_bytes!("../assets/fonts/Manrope-VF.ttf");

fn wght(typeface: &Typeface) -> Option<f32> {
    let tag = FourByteTag::from_chars('w', 'g', 'h', 't');
    typeface
        .variation_design_position()?
        .iter()
        .find(|coordinate| coordinate.axis == tag)
        .map(|coordinate| coordinate.value)
}

fn advance(typeface: &Typeface, text: &str) -> f32 {
    Font::from_typeface(typeface.clone(), 32.0)
        .measure_str(text, None)
        .0
}

#[test]
fn variable_font_serves_requested_weight() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    jian_skia::remove_imported_font("Inter");
    jian_skia::register_imported_font(INTER_VF.to_vec()).expect("register Inter");
    let resolver = FontResolver::new(FontMgr::new());

    let mut widths = Vec::new();
    for weight in (1..=9u16).map(|step| step * 100) {
        let face = resolver
            .typeface_for_char(Some("Inter"), 'H', weight, false)
            .expect("downloaded face");
        assert_eq!(face.typeface.family_name(), "Inter");
        assert_eq!(
            *face.typeface.font_style().weight(),
            i32::from(weight),
            "weight {weight} must resolve to a face that reports it"
        );
        assert_eq!(
            wght(&face.typeface),
            Some(f32::from(weight)),
            "axis at {weight}"
        );
        assert!(
            !face.synthetic_bold,
            "a real instance needs no stroke (weight {weight})"
        );
        assert!(!face.synthetic_italic);
        widths.push(advance(&face.typeface, "Hamburgefons"));
    }
    assert!(
        widths.windows(2).all(|pair| pair[0] < pair[1]),
        "heavier instances are wider, so they really are different outlines: {widths:?}"
    );

    // An italic request on an upright-only file keeps the face and skews it.
    let italic = resolver
        .typeface_for_char(Some("Inter"), 'H', 700, true)
        .expect("italic request");
    assert!(italic.synthetic_italic);
    assert_eq!(wght(&italic.typeface), Some(700.0));
    jian_skia::remove_imported_font("Inter");
}

#[test]
fn variable_range_is_clamped_to_what_the_axis_offers() {
    let _registry = crate::font_download_host::font_registry_test_lock();
    jian_skia::remove_imported_font("Manrope");
    jian_skia::register_imported_font(MANROPE_VF.to_vec()).expect("register Manrope");
    let resolver = FontResolver::new(FontMgr::new());
    // Manrope's wght axis is 200..800: 100 and 900 snap to the nearest end.
    for (requested, expected) in [
        (100, 200.0),
        (200, 200.0),
        (500, 500.0),
        (800, 800.0),
        (900, 800.0),
    ] {
        let face = resolver
            .typeface_for_char(Some("Manrope"), 'H', requested, false)
            .expect("downloaded face");
        assert_eq!(
            wght(&face.typeface),
            Some(expected),
            "requested {requested}"
        );
    }
    assert_eq!(
        jian_skia::list_families()
            .iter()
            .find(|meta| meta.family == "Manrope")
            .map(|meta| meta.face_count),
        Some(1),
        "the instances are views of one registered file"
    );
    jian_skia::remove_imported_font("Manrope");
}
