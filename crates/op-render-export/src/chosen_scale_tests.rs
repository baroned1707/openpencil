//! `render_node_raster_at_chosen_scale`: the scale the caller picks is the
//! scale drawn, unlike the export entry points that clamp it to 0.5..8.

use super::test_support::{filled_rect, scene_with};
use super::*;
use op_editor_ui::Color;

fn png_size(bytes: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
    )
}

#[test]
fn the_chosen_scale_is_drawn_and_reported_without_the_export_floor() {
    let scene = scene_with(vec![filled_rect("n", 0.0, 0.0, 80.0, 40.0, Color::BLACK)]);
    let mut seen = None;
    let raster = render_node_raster_at_chosen_scale(&scene, "n", RasterFormat::Png, |w, h| {
        seen = Some((w, h));
        0.25
    })
    .expect("render");
    assert_eq!(seen, Some((80.0, 40.0)));
    assert_eq!(
        (raster.scale, raster.width_px, raster.height_px),
        (0.25, 20, 10)
    );
    assert_eq!(png_size(&raster.bytes), (20, 10));

    // The export entry point would have lifted 0.25 to 0.5.
    let exported = render_node_raster_bytes(&scene, "n", RasterFormat::Png, 0.25).expect("export");
    assert_eq!(png_size(&exported), (40, 20));
}

#[test]
fn an_unusable_scale_and_unknown_nodes_are_refused() {
    let scene = scene_with(vec![filled_rect("n", 0.0, 0.0, 10.0, 10.0, Color::BLACK)]);
    for bad in [0.0_f32, -1.0, f32::NAN, f32::INFINITY] {
        let result = render_node_raster_at_chosen_scale(&scene, "n", RasterFormat::Png, |_, _| bad);
        assert!(
            matches!(result, Err(ExportError::NonFiniteOutputSize)),
            "{bad}"
        );
    }
    let missing =
        render_node_raster_at_chosen_scale(&scene, "ghost", RasterFormat::Png, |_, _| 1.0);
    assert!(matches!(
        missing,
        Err(ExportError::NodeNotFoundOnPage { .. })
    ));
}
