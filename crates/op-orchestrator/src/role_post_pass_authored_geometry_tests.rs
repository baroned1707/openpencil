//! Post-pass fixes must not overrule authored geometry: a screen root keeps
//! its viewport height, and a category chip keeps its authored size / radius.

use super::*;
use op_editor_core::PenNodeExt;
use serde_json::json;

/// The three-argument shape the other normalizer tests use (full tier, no
/// known root form).
fn post_pass_value(node: &mut Value, parent_fill: Option<Value>, canvas_width: f64) {
    super::post_pass_value(
        node,
        parent_fill,
        canvas_width,
        true,
        DesignForm::Unknown,
        &mut Vec::new(),
    );
}

fn screen(id: &str, width: u32, height: u32) -> serde_json::Value {
    json!({
        "type":"frame","id":id,"name":id,"width":width,"height":height,
        "layout":"vertical","clipContent":true,
        "fill":[{"type":"solid","color":"#FFFFFF"}],
        "children":[
            {"type":"text","id":format!("{id}-title"),"content":"Welcome"},
            {"type":"frame","id":format!("{id}-cta"),"role":"button","name":"Continue Button",
             "width":"fill_container","height":52,
             "fill":[{"type":"solid","color":"#0369A1"}],
             "children":[{"type":"text","id":format!("{id}-cta-label"),"content":"Continue",
                          "fill":[{"type":"solid","color":"#FFFFFF"}]}]}
        ]
    })
}

fn promo_card() -> serde_json::Value {
    json!({
        "type":"frame","id":"promo","role":"banner","name":"Limited Offer Promo Card",
        "layout":"horizontal","width":342,"height":156,"clipContent":true,
        "children":[
            {"type":"text","id":"p1","content":"50% OFF"},
            {"type":"frame","id":"p2","role":"button","name":"Order Now Button",
             "children":[{"type":"text","id":"p3","content":"Order Now"}]}
        ]
    })
}

fn forest(values: Vec<serde_json::Value>) -> Vec<PenNode> {
    values
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("fixture must deserialize"))
        .collect()
}

#[test]
fn screen_roots_with_a_button_keep_their_viewport_height() {
    let mut nodes = forest(vec![
        screen("one", 390, 844),
        screen("two", 375, 812),
        screen("desktop", 1440, 900),
    ]);
    post_pass_forest(&mut nodes, 390.0);
    let heights: Vec<_> = nodes.iter().map(|node| node.height_px()).collect();
    assert_eq!(heights, vec![Some(844.0), Some(812.0), Some(900.0)]);
}

#[test]
fn three_phone_screens_stay_three_screens_after_finalize() {
    use op_editor_core::EditorState;
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version":"1.0",
        "children":[screen("one", 390, 844), screen("two", 390, 844), screen("three", 390, 844)]
    }))
    .expect("valid doc");
    let mut state = EditorState::from_document(doc);

    crate::loop_finalize::apply_loop_finalize_counted(&mut state);

    for node in state.active_children() {
        assert_eq!(node.height_px(), Some(844.0), "{}", node.id_str());
    }
    assert_eq!(
        crate::unfilled_screens::list_screen_candidates(&state).len(),
        3
    );
}

#[test]
fn a_clipped_promo_card_inside_a_screen_still_relaxes() {
    let mut root = screen("one", 390, 844);
    root["children"].as_array_mut().unwrap().push(promo_card());
    let mut nodes = forest(vec![root]);
    post_pass_forest(&mut nodes, 390.0);
    let value = serde_json::to_value(&nodes[0]).unwrap();
    assert_eq!(nodes[0].height_px(), Some(844.0));
    let promo = value["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["id"] == "promo")
        .expect("promo kept");
    assert_eq!(promo["height"], json!("fit_content"));
}

#[test]
fn a_short_full_width_hero_with_a_cta_still_relaxes() {
    let mut hero = screen("hero", 1440, 600);
    hero["role"] = json!("hero");
    let mut nodes = forest(vec![hero]);
    post_pass_forest(&mut nodes, 1440.0);
    assert_eq!(
        serde_json::to_value(&nodes[0]).unwrap()["height"],
        json!("fit_content")
    );
}

// ── mobile category chips ───────────────────────────────────────────────────

fn chip(id: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut chip = json!({
        "type":"frame","id":id,"role":"chip","name":format!("{id} Category"),
        "children":[{"type":"text","id":format!("{id}-label"),"content":id}]
    });
    for (key, value) in extra.as_object().unwrap() {
        chip[key] = value.clone();
    }
    chip
}

#[test]
fn category_chips_keep_authored_size_and_radius() {
    let mut row = json!({
        "type":"frame","id":"rail","layout":"horizontal","name":"Category Chips",
        "children":[
            chip("pizza", json!({"width":96,"height":40,"cornerRadius":20})),
            chip("sushi", json!({"width":88,"height":40,"cornerRadius":[20, 20, 4, 4]})),
            chip("tacos", json!({"width":80,"height":40,"cornerRadius":0})),
            chip("salad", json!({"width":84,"height":40,"cornerRadius":20}))
        ]
    });
    post_pass_value(&mut row, Some(Value::Null), 390.0);
    let chips = row["children"].as_array().unwrap();
    assert_eq!(
        chips.iter().map(|c| c["width"].clone()).collect::<Vec<_>>(),
        vec![json!(96), json!(88), json!(80), json!(84)]
    );
    assert!(chips.iter().all(|c| c["height"] == json!(40)));
    assert_eq!(chips[0]["cornerRadius"], json!(20));
    assert_eq!(chips[1]["cornerRadius"], json!([20, 20, 4, 4]));
    assert_eq!(chips[2]["cornerRadius"], json!(0));
}

#[test]
fn category_chips_without_geometry_are_filled_in() {
    let mut row = json!({
        "type":"frame","id":"rail","layout":"horizontal","name":"Category Chips",
        "children":[
            chip("a", json!({})), chip("b", json!({"width":"fill_container"})),
            chip("c", json!({})), chip("d", json!({}))
        ]
    });
    post_pass_value(&mut row, Some(Value::Null), 390.0);
    for chip in row["children"].as_array().unwrap() {
        assert_eq!(chip["width"], json!("fit_content"));
        assert_eq!(chip["height"], json!("fit_content"));
        assert_eq!(chip["cornerRadius"], json!(8));
    }
}
