//! The text-contrast repair judges text only against ONE opaque solid
//! backdrop. Over an image, a gradient or a translucent overlay it leaves the
//! text as authored (the write-time scan reports those as uncertain).

use super::*;
use serde_json::json;

fn dark_text(id: &str) -> serde_json::Value {
    json!({"type":"text","id":id,"content":"Welcome","fill":[{"type":"solid","color":"#111111"}]})
}

fn text_fill(frame: &serde_json::Value, index: usize) -> serde_json::Value {
    frame["children"][index]["fill"].clone()
}

fn dark_fill() -> serde_json::Value {
    json!([{"type":"solid","color":"#111111"}])
}

fn white_fill() -> serde_json::Value {
    json!([{"type":"solid","color":"#FFFFFF"}])
}

fn image_fill() -> serde_json::Value {
    json!({"type":"image","url":"https://example.test/hero.jpg"})
}

/// Invariant: an opaque solid backdrop keeps today's behaviour, light or dark.
#[test]
fn dark_text_on_an_opaque_dark_solid_is_still_recoloured() {
    for bg in ["#0F172A", "#111111"] {
        let mut frame = json!({
            "type":"frame","id":"f","fill":[{"type":"solid","color":bg}],
            "children":[dark_text("t")]
        });
        fix_container_text_contrast(&mut frame);
        assert_eq!(text_fill(&frame, 0), white_fill(), "{bg}");
    }
}

#[test]
fn text_over_an_image_fill_is_not_recoloured() {
    let mut frame = json!({
        "type":"frame","id":"hero","fill":[image_fill()],
        "children":[dark_text("t")]
    });
    fix_container_text_contrast(&mut frame);
    assert_eq!(text_fill(&frame, 0), dark_fill());
}

#[test]
fn an_image_under_a_forty_percent_black_overlay_is_not_treated_as_solid_black() {
    let mut frame = json!({
        "type":"frame","id":"hero",
        "fill":[image_fill(), {"type":"solid","color":"#000000","opacity":0.4}],
        "children":[dark_text("t")]
    });
    fix_container_text_contrast(&mut frame);
    assert_eq!(text_fill(&frame, 0), dark_fill());
}

#[test]
fn text_over_a_gradient_is_not_recoloured() {
    let mut frame = json!({
        "type":"frame","id":"hero",
        "fill":[{"type":"linear_gradient","angle":90,"stops":[
            {"position":0,"color":"#000000"},{"position":1,"color":"#FFFFFF"}]}],
        "children":[dark_text("t")]
    });
    fix_container_text_contrast(&mut frame);
    assert_eq!(text_fill(&frame, 0), dark_fill());
}

#[test]
fn text_over_a_translucent_solid_is_not_recoloured() {
    for fill in [
        json!({"type":"solid","color":"#000000","opacity":0.4}),
        json!({"type":"solid","color":"#00000066"}),
    ] {
        let mut frame = json!({
            "type":"frame","id":"glass","fill":[fill],
            "children":[dark_text("t")]
        });
        fix_container_text_contrast(&mut frame);
        assert_eq!(text_fill(&frame, 0), dark_fill());
    }
}

/// An outer opaque white frame would turn the text inside a nested
/// image-with-overlay frame dark; the nested frame must stop that walk.
#[test]
fn a_nested_image_overlay_frame_shields_its_text_from_the_outer_colour() {
    let white_text = json!({
        "type":"text","id":"t","content":"On photo","fill":[{"type":"solid","color":"#FFFFFF"}]
    });
    let mut outer = json!({
        "type":"frame","id":"page","fill":[{"type":"solid","color":"#FFFFFF"}],
        "children":[{
            "type":"frame","id":"hero",
            "fill":[image_fill(), {"type":"solid","color":"#000000","opacity":0.4}],
            "children":[white_text]
        }]
    });
    fix_container_text_contrast(&mut outer);
    assert_eq!(outer["children"][0]["children"][0]["fill"], white_fill());
}

/// A nested OPAQUE solid frame is still judged against its own colour.
#[test]
fn a_nested_opaque_solid_frame_is_still_judged() {
    let mut outer = json!({
        "type":"frame","id":"page","fill":[{"type":"solid","color":"#FFFFFF"}],
        "children":[{
            "type":"frame","id":"card","fill":[{"type":"solid","color":"#0F172A"}],
            "children":[dark_text("t")]
        }]
    });
    fix_container_text_contrast(&mut outer);
    assert_eq!(outer["children"][0]["children"][0]["fill"], white_fill());
}

#[test]
fn a_button_over_an_image_keeps_its_content_colours() {
    let mut button = json!({
        "type":"frame","role":"button","fill":[image_fill()],
        "children":[{"type":"icon_font","iconFontName":"arrow-right",
            "fill":[{"type":"solid","color":"#0F172A"}]}]
    });
    fix_button_foreground_contrast(&mut button);
    assert_eq!(
        button["children"][0]["fill"],
        json!([{"type":"solid","color":"#0F172A"}])
    );
}

// ── WCAG-based text contrast repair ─────────────────────────────────────────

fn label(color: &str, size: u32) -> serde_json::Value {
    json!({"type":"text","content":"42","fontSize":size,"fill":[{"type":"solid","color":color}]})
}

fn bar(children: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"type":"frame","name":"Bar Fill","fill":[{"type":"solid","color":"#3B82F6"}],"children":children})
}

fn colour_of(frame: &serde_json::Value, index: usize) -> String {
    frame["children"][index]["fill"][0]["color"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Regression: a Rec.601 luma rule flipped this label to white (3.68:1) even
/// though the authored colour already reads at 4.79:1.
#[test]
fn authored_text_that_meets_the_ratio_is_kept() {
    use op_design_lint::color::color_contrast;
    assert!(color_contrast("#17191D", "#3B82F6") >= 4.5);
    assert!(color_contrast("#FFFFFF", "#3B82F6") < 4.5);
    // A sibling with a higher ratio must not pull the passing label along.
    let mut frame = bar(vec![label("#17191D", 12), label("#000000", 12)]);
    fix_container_text_contrast(&mut frame);
    assert_eq!(colour_of(&frame, 0), "#17191D");
    assert_eq!(colour_of(&frame, 1), "#000000");
}

#[test]
fn authored_text_below_the_ratio_is_recoloured_to_one_that_passes() {
    use op_design_lint::color::color_contrast;
    let mut frame = bar(vec![label("#FFFFFF", 12)]);
    fix_container_text_contrast(&mut frame);
    let repaired = colour_of(&frame, 0);
    assert_eq!(repaired, "#0F172A");
    assert!(color_contrast(&repaired, "#3B82F6") >= 4.5);
}

#[test]
fn large_text_only_needs_three_to_one() {
    let mut frame = bar(vec![label("#FFFFFF", 28)]);
    fix_container_text_contrast(&mut frame);
    assert_eq!(colour_of(&frame, 0), "#FFFFFF");
    // The same colour at body size fails 4.5:1 and is recoloured.
    let mut body = bar(vec![label("#FFFFFF", 14)]);
    fix_container_text_contrast(&mut body);
    assert_ne!(colour_of(&body, 0), "#FFFFFF");
}

#[test]
fn a_passing_colour_already_in_the_container_is_preferred() {
    let mut frame = json!({
        "type":"frame","fill":[{"type":"solid","color":"#FFFFFF"}],
        "children":[label("#374151", 14), label("#EEEEEE", 14)]
    });
    fix_container_text_contrast(&mut frame);
    assert_eq!(colour_of(&frame, 1), "#374151");
}

#[test]
fn pure_black_is_the_last_resort_when_the_repo_dark_and_white_both_fail() {
    use op_design_lint::color::color_contrast;
    // #0F172A and white both stay under 4.5:1 on this mid grey.
    assert!(color_contrast("#0F172A", "#7B7B7B") < 4.5);
    assert!(color_contrast("#FFFFFF", "#7B7B7B") < 4.5);
    let mut frame = json!({
        "type":"frame","fill":[{"type":"solid","color":"#7B7B7B"}],
        "children":[label("#FFFFFF", 14)]
    });
    fix_container_text_contrast(&mut frame);
    assert_eq!(colour_of(&frame, 0), "#000000");
}
