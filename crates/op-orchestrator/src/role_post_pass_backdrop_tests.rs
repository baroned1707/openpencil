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
