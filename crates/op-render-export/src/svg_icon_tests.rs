//! Export of an Iconify icon: an SVG data URL on an image node.

use super::test_support::scene_with;
use super::tests::{decode_rgba, pixel_at};
use super::*;
use op_editor_ui::layout_scene::{NodeKind, SceneNode};
use op_editor_ui::{Color, Rect};

/// An Iconify icon lands as `data:image/svg+xml;base64,...`. Export must
/// rasterize it at the shared byte-cache seam (`svg_raster`), not paint the
/// placeholder: the icon's `currentColor` square covers the node centre.
#[test]
fn export_renders_an_svg_data_url_icon() {
    use base64::Engine as _;
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 24 24"><path fill="currentColor" d="M3 3h18v18H3z"/></svg>"#;
    let mut node = SceneNode::leaf("icon", NodeKind::Rect);
    node.bounds = Rect::xywh(0.0, 0.0, 24.0, 24.0);
    node.image_src = Some(
        format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        )
        .into(),
    );
    node.fill = Some(Color {
        r: 0.85,
        g: 0.86,
        b: 0.88,
        a: 1.0,
    });
    let scene = scene_with(vec![node]);
    let tmp = std::env::temp_dir().join(format!("op-export-svg-icon-{}.png", std::process::id()));
    let res = export_node_raster(&scene, "icon", &tmp, RasterFormat::Png, 1.0);
    assert!(res.is_ok(), "svg icon export failed: {res:?}");
    let decoded = decode_rgba(&std::fs::read(&tmp).unwrap());
    let centre = pixel_at(&decoded, 12, 12);
    assert!(
        centre[0] < 60 && centre[1] < 60 && centre[2] < 60 && centre[3] > 200,
        "expected the dark icon square at the node centre, got {centre:?}"
    );
    let _ = std::fs::remove_file(&tmp);
}
