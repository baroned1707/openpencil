//! Size cap and honest scale of the inline `get_screenshot`.

use super::*;
use crate::export::test_support::{filled_rect, scene_with};
use op_editor_ui::Color;

fn tool_for_frame(width: f32, height: f32) -> GetScreenshot {
    get_screenshot_from_scene(scene_with(vec![filled_rect(
        "n1",
        0.0,
        0.0,
        width,
        height,
        Color::BLACK,
    )]))
}

fn shot(tool: &GetScreenshot, scale: Option<&str>) -> (u32, u32, serde_json::Value) {
    let mut args = BTreeMap::new();
    args.insert("nodeId".to_string(), "n1".to_string());
    if let Some(scale) = scale {
        args.insert("scale".to_string(), scale.to_string());
    }
    let ToolOutcome::OkImageContent {
        image_base64,
        metadata_json,
        ..
    } = tool.call(&args)
    else {
        panic!("expected a screenshot");
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(image_base64)
        .expect("base64");
    // PNG IHDR: width and height are the big-endian u32s at bytes 16 and 20.
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let meta = serde_json::from_str(&metadata_json.expect("metadata")).expect("json");
    (width, height, meta)
}

/// Invariant: whatever the cap does, the reported width, height and scale
/// describe the PNG that was returned.
fn assert_report_matches_pixels(
    width: u32,
    height: u32,
    meta: &serde_json::Value,
    frame: (f32, f32),
) {
    assert_eq!(meta["width"], width, "{meta}");
    assert_eq!(meta["height"], height, "{meta}");
    assert_eq!(meta["size"], format!("{width}x{height}"), "{meta}");
    let scale = meta["scale"].as_f64().expect("scale") as f32;
    assert_eq!((frame.0 * scale).round() as u32, width, "{meta}");
    assert_eq!((frame.1 * scale).round() as u32, height, "{meta}");
}

#[test]
fn a_4000_by_3000_frame_comes_back_within_the_default_long_edge() {
    let (width, height, meta) = shot(&tool_for_frame(4000.0, 3000.0), None);
    assert!(width.max(height) <= 1568, "{width}x{height}");
    assert_eq!((width, height), (1568, 1176));
    assert_report_matches_pixels(width, height, &meta, (4000.0, 3000.0));
    assert_eq!(meta["requestedScale"], 1.0);
    let hint = meta["hint"].as_str().expect("downscale hint");
    assert!(
        hint.contains("4000x3000") && hint.contains("0.392"),
        "{hint}"
    );
    assert!(hint.contains("crop to a child node"), "{hint}");
}

#[test]
fn a_tall_page_is_capped_on_its_height() {
    let (width, height, meta) = shot(&tool_for_frame(1440.0, 6000.0), None);
    assert!(height <= 1568 && height > 1500, "{width}x{height}");
    assert_report_matches_pixels(width, height, &meta, (1440.0, 6000.0));
}

#[test]
fn a_small_frame_is_unchanged_and_carries_no_hint() {
    let (width, height, meta) = shot(&tool_for_frame(80.0, 40.0), None);
    assert_eq!((width, height), (80, 40));
    assert_eq!(meta["scale"], 1.0);
    assert!(meta.get("hint").is_none(), "{meta}");
    assert_report_matches_pixels(width, height, &meta, (80.0, 40.0));
}

#[test]
fn a_quarter_scale_request_is_drawn_and_reported_at_a_quarter() {
    // The export path floors the scale at 0.5; the screenshot must not, or
    // the reported 0.25 would describe a 0.5 image.
    let (width, height, meta) = shot(&tool_for_frame(80.0, 80.0), Some("0.25"));
    assert_eq!((width, height), (20, 20));
    assert_eq!(meta["scale"], 0.25);
    assert_eq!(meta["requestedScale"], 0.25);
    assert!(meta.get("hint").is_none(), "{meta}");
}

#[test]
fn an_explicit_scale_is_honoured_above_the_default_cap() {
    let (width, height, meta) = shot(&tool_for_frame(2000.0, 1000.0), Some("1"));
    assert_eq!((width, height), (2000, 1000));
    assert_eq!(meta["scale"], 1.0);
    assert!(meta.get("hint").is_none(), "{meta}");
}

#[test]
fn an_explicit_scale_stops_at_the_hard_long_edge_cap() {
    let (width, height, meta) = shot(&tool_for_frame(4000.0, 3000.0), Some("2"));
    assert!(width.max(height) <= 2576, "{width}x{height}");
    assert!(width > 2500, "the hard cap, not the default one: {width}");
    assert_report_matches_pixels(width, height, &meta, (4000.0, 3000.0));
    assert_eq!(meta["requestedScale"], 2.0);
    let hint = meta["hint"].as_str().expect("hint");
    assert!(hint.contains("2576"), "{hint}");
}

#[test]
fn plan_scale_floors_so_the_rounded_edge_never_exceeds_the_cap() {
    for long_edge in [1569.0_f32, 1700.0, 3333.0, 4000.0, 9999.0, 16000.0] {
        let plan = plan_scale(None, long_edge, 100.0);
        assert!(plan.downscaled);
        assert!(
            (long_edge * plan.used).round() <= DEFAULT_LONG_EDGE_PX,
            "{long_edge}: {plan:?}"
        );
    }
    assert!(!plan_scale(None, 1568.0, 10.0).downscaled);
}

#[test]
fn a_small_icon_is_enlarged_to_the_requested_scale() {
    let (width, height, meta) = shot(&tool_for_frame(24.0, 24.0), Some("16"));
    assert_eq!((width, height), (384, 384));
    assert_eq!(meta["scale"], 16.0);
    assert_eq!(meta["requestedScale"], 16.0);
    assert!(meta.get("hint").is_none(), "{meta}");
    assert_report_matches_pixels(width, height, &meta, (24.0, 24.0));
}

#[test]
fn a_high_scale_still_stops_at_the_hard_long_edge_cap() {
    // 400 px at scale 16 would be 6400 px; the cap lowers it to 2576 and the
    // reported scale is the one used.
    let (width, height, meta) = shot(&tool_for_frame(400.0, 200.0), Some("16"));
    assert_eq!(width, 2576);
    assert_report_matches_pixels(width, height, &meta, (400.0, 200.0));
    assert_eq!(meta["requestedScale"], 16.0);
    assert!(meta["hint"].as_str().expect("hint").contains("2576"));
}

#[test]
fn a_node_already_at_the_hard_cap_is_not_enlarged() {
    let (width, height, meta) = shot(&tool_for_frame(2576.0, 100.0), Some("8"));
    assert_eq!((width, height), (2576, 100));
    assert_eq!(meta["scale"], 1.0);
    assert_eq!(meta["requestedScale"], 8.0);
}

#[test]
fn omitted_and_low_scales_keep_the_pre_raise_plan() {
    // Byte-identical behaviour for scale omitted or <= 2: same plan as before
    // the ceiling moved from 2 to 16.
    let plan = plan_scale(None, 4000.0, 3000.0);
    assert_eq!(
        (plan.used, plan.requested, plan.hard_cap),
        (0.392, 1.0, false)
    );
    let plan = plan_scale(Some(2.0), 4000.0, 3000.0);
    assert!(plan.downscaled && plan.hard_cap);
    assert_eq!(plan.used, 0.644);
    let plan = plan_scale(Some(2.0), 80.0, 80.0);
    assert_eq!((plan.used, plan.downscaled), (2.0, false));
}
