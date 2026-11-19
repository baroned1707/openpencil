//! A flat forest (several top-level frames, no page frame) has no page
//! background to read a theme from. Reading it as Light let the text-band
//! heuristic paint a fill-less dark sidebar with the accent colour.

use super::*;
use op_editor_core::EditorState;
use serde_json::json;

fn nodes(values: Vec<serde_json::Value>) -> Vec<PenNode> {
    values
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("fixture must deserialize as PenNode"))
        .collect()
}

fn frame(id: &str, width: u32) -> serde_json::Value {
    json!({
        "type":"frame","id":id,"name":id,"width":width,"height":600,"layout":"vertical",
        "children":[{"type":"text","id":format!("{id}-t"),"content":"x"}]
    })
}

#[test]
fn two_top_level_frames_have_no_known_theme() {
    let forest = nodes(vec![frame("a", 240), frame("b", 1200)]);
    let (has_wrapper, page_bg, _, theme) = locate_section_context(&forest);
    assert!(!has_wrapper);
    assert_eq!(page_bg, None);
    assert_eq!(theme, Theme::Unknown);
}

#[test]
fn a_page_frame_with_a_known_background_still_gives_a_theme() {
    let mut page = frame("page", 1200);
    page["fill"] = json!([{"type":"solid","color":"#FFFFFF"}]);
    let (has_wrapper, _, _, theme) = locate_section_context(&nodes(vec![page]));
    assert!(has_wrapper);
    assert_eq!(theme, Theme::Light);
}

/// `Sidebar` (white labels, no fill of its own) and `Main` as two top-level
/// frames: the sidebar must not receive a fill from the text-band heuristic.
#[test]
fn a_fill_less_sidebar_with_white_text_gets_no_fill() {
    let sidebar = json!({
        "type":"frame","id":"side","name":"Sidebar","width":240,"height":900,"layout":"vertical",
        "children":[
            {"type":"text","id":"s1","content":"Overview","fill":[{"type":"solid","color":"#FFFFFF"}]},
            {"type":"text","id":"s2","content":"Clients","fill":[{"type":"solid","color":"#FFFFFF"}]}
        ]
    });
    let main = json!({
        "type":"frame","id":"main","name":"Main","width":1200,"height":900,"layout":"vertical",
        "fill":[{"type":"solid","color":"#0B1220"}],
        "children":[{"type":"text","id":"m1","content":"Revenue","fill":[{"type":"solid","color":"#FFFFFF"}]}]
    });
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version":"1.0","children":[sidebar, main]
    }))
    .expect("valid doc");
    let mut state = EditorState::from_document(doc);

    apply_loop_finalize_counted(&mut state);

    let side = state
        .active_children()
        .iter()
        .find(|node| node.id_str() == "side")
        .expect("sidebar kept");
    let fill = serde_json::to_value(side).unwrap()["fill"].clone();
    assert!(
        fill.is_null() || fill == json!([]),
        "sidebar was painted: {fill}"
    );
}
