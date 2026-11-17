//! `snapshot_layout` marks clipped and collapsed nodes, and only those.

use std::collections::BTreeMap;

use op_editor_core::EditorState;
use serde_json::{json, Value};

use crate::*;

fn layout_of(children: Value) -> Value {
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version": "1.0",
        "children": [{ "type": "frame", "id": "root", "name": "Root", "width": 400,
                       "height": 400, "children": children }]
    }))
    .expect("document");
    let state = EditorState::from_document(doc);
    let mut registry = ToolRegistry::default();
    registry.register(Box::new(snapshot_layout_snapshot(&state)));
    let arguments = BTreeMap::from([("maxDepth".to_string(), "4".to_string())]);
    match registry.dispatch(ToolCall {
        id: RequestId::Num(1),
        tool: "snapshot_layout".into(),
        arguments,
    }) {
        ToolResponse::Ok {
            json: Some(raw), ..
        } => serde_json::from_str(&raw).expect("layout"),
        other => panic!("expected OkJson layout, got {other:?}"),
    }
}

/// Find an entry by id anywhere in the layout tree.
fn entry<'a>(layout: &'a Value, id: &str) -> &'a Value {
    fn search<'a>(nodes: &'a Value, id: &str) -> Option<&'a Value> {
        nodes.as_array()?.iter().find_map(|node| {
            if node["id"] == id {
                Some(node)
            } else {
                search(&node["children"], id)
            }
        })
    }
    search(&layout["layout"], id).unwrap_or_else(|| panic!("no entry {id} in {layout}"))
}

fn rect(id: &str, x: i32, y: i32, width: i32, height: i32) -> Value {
    json!({ "type": "rectangle", "id": id, "x": x, "y": y, "width": width, "height": height,
            "fill": [{ "type": "solid", "color": "#cccccc" }] })
}

/// Invariant: a node whose bounds extend beyond a parent that clips its content
/// is `partial` when some of it still shows and `full` when none does; a node
/// that fits, or sits in a parent that does not clip, carries no key. A frame
/// or text with a zero side is `collapsed`.
#[test]
fn snapshot_layout_marks_clipped_and_collapsed_nodes() {
    let layout = layout_of(json!([
        { "type": "frame", "id": "clipper", "x": 0, "y": 0, "width": 100, "height": 100,
          "clipContent": true, "layout": "none", "children": [
            rect("inside", 10, 10, 20, 20),
            rect("partial", 80, 10, 50, 20),
            rect("outside", 150, 0, 20, 20)
        ] },
        { "type": "frame", "id": "loose", "x": 0, "y": 150, "width": 100, "height": 50,
          "clipContent": false, "layout": "none", "children": [rect("spill", 80, 0, 60, 20)] },
        { "type": "frame", "id": "flat", "x": 200, "y": 0, "width": 100, "height": 0,
          "layout": "none", "children": [] }
    ]));
    assert!(
        entry(&layout, "inside").get("clipped").is_none(),
        "{layout}"
    );
    assert_eq!(entry(&layout, "partial")["clipped"], "partial", "{layout}");
    assert_eq!(entry(&layout, "outside")["clipped"], "full", "{layout}");
    assert!(entry(&layout, "spill").get("clipped").is_none(), "{layout}");
    assert_eq!(entry(&layout, "flat")["collapsed"], true, "{layout}");
    assert!(
        entry(&layout, "clipper").get("collapsed").is_none(),
        "{layout}"
    );
}

/// Invariant: a tree that fits has no `clipped` / `collapsed` key anywhere, so
/// its output is byte-for-byte what it was before the keys existed.
#[test]
fn snapshot_layout_clean_tree_is_unchanged() {
    let layout = layout_of(json!([
        { "type": "frame", "id": "card", "x": 10, "y": 10, "width": 200, "height": 100,
          "clipContent": true, "layout": "none", "children": [
            rect("a", 0, 0, 100, 50),
            rect("b", 100, 50, 100, 50),
            // Half a pixel past the edge is layout rounding, not clipping.
            rect("edge", 150, 0, 50.4_f64 as i32, 10)
        ] }
    ]));
    let raw = layout.to_string();
    assert!(
        !raw.contains("clipped") && !raw.contains("collapsed"),
        "{raw}"
    );
    let expected_keys = [
        "children", "height", "id", "name", "type", "width", "x", "y",
    ];
    for id in ["root", "card", "a", "b"] {
        let mut keys: Vec<&str> = entry(&layout, id)
            .as_object()
            .expect("entry object")
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "children" || id == "root" || id == "card")
            .collect();
        keys.sort_unstable();
        assert!(
            keys.iter().all(|key| expected_keys.contains(key)),
            "{id}: unexpected keys {keys:?}"
        );
    }
}
