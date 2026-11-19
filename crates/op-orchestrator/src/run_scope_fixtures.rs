//! Shared fixtures for the run-scope tests: a landing page shaped like eval
//! case `09-modify-hero-bolder` (one page wrapper, navigation, hero, features
//! with three cards), plus helpers that play the part of an agent run.

use jian_ops_schema::node::PenNode;
use op_editor_core::{EditorCommand, EditorState, NodeId};
use serde_json::{json, Value};

pub(crate) fn text(id: &str, content: &str, size: u32, color: &str) -> Value {
    json!({
        "type": "text", "id": id, "content": content, "fontFamily": "Inter",
        "fontSize": size, "fontWeight": "500",
        "width": "fit_content", "height": "fit_content",
        "fill": [{ "type": "solid", "color": color }]
    })
}

pub(crate) fn card(id: &str, title: &str) -> Value {
    json!({
        "type": "frame", "id": id, "name": title, "layout": "vertical", "gap": 6,
        "width": "fill_container", "height": "fit_content",
        "children": [
            text(&format!("{id}-t"), title, 18, "#111827"),
            text(&format!("{id}-b"), "Short supporting copy.", 15, "#374151"),
        ]
    })
}

/// The page: `page` > `nav`, `hero`, `features` (> `features-row` > `f1..f3`).
/// `nav_ink` is the colour of the logo text, so a test can plant a defect.
pub(crate) fn page_json(nav_ink: &str) -> Value {
    json!({
        "version": "1.0",
        // The palette the contrast repair re-points a fill at (it never
        // invents a colour), plus one the page binds its logo to.
        "variables": {
            "--ink": { "type": "color", "value": "#111827" },
            "--foreground": { "type": "color", "value": "#111827" },
            "--background": { "type": "color", "value": "#FFFFFF" },
            "--muted-foreground": { "type": "color", "value": "#4B5563" }
        },
        "children": [{
            "type": "frame", "id": "page", "name": "Studio Landing",
            "layout": "vertical", "width": 1440, "height": 1100,
            "fill": [{ "type": "solid", "color": "#FFFFFF" }],
            "children": [
                {
                    "type": "frame", "id": "nav", "name": "Navigation",
                    "layout": "horizontal", "width": "fill_container",
                    "height": "fit_content", "justifyContent": "space_between",
                    "alignItems": "center", "padding": [20, 80, 20, 80],
                    "children": [
                        text("nav-logo", "Fieldnotes", 22, nav_ink),
                        { "type": "frame", "id": "nav-links", "name": "Links",
                          "layout": "horizontal", "gap": 24,
                          "width": "fit_content", "height": "fit_content",
                          "children": [
                              text("nav-l1", "Work", 15, "#374151"),
                              text("nav-l2", "About", 15, "#374151"),
                          ] }
                    ]
                },
                {
                    "type": "frame", "id": "hero", "name": "Hero",
                    "layout": "vertical", "gap": 18, "width": "fill_container",
                    "height": "fit_content", "padding": [96, 80, 96, 80],
                    "fill": [{ "type": "solid", "color": "#F3F4F6" }],
                    "children": [
                        text("hero-eyebrow", "DESIGN STUDIO", 13, "#6B7280"),
                        text("hero-title", "We design calm software.", 56, "#374151"),
                        text("hero-sub", "A small studio for product teams.", 18, "#4B5563"),
                    ]
                },
                {
                    "type": "frame", "id": "features", "name": "Features",
                    "layout": "vertical", "gap": 24, "width": "fill_container",
                    "height": "fit_content", "padding": [64, 80, 64, 80],
                    "children": [
                        text("features-title", "What we do", 32, "#111827"),
                        { "type": "frame", "id": "features-row", "layout": "horizontal",
                          "gap": 32, "width": "fill_container", "height": "fit_content",
                          "children": [
                              card("f1", "Strategy"),
                              card("f2", "Interface"),
                              card("f3", "Systems"),
                          ] }
                    ]
                }
            ]
        }]
    })
}

pub(crate) fn state_from(doc: Value) -> EditorState {
    let doc = serde_json::from_value(doc).expect("fixture document");
    EditorState::from_document(doc)
}

pub(crate) fn page() -> EditorState {
    state_from(page_json("#111827"))
}

/// What an agent's `U(id, patch)` does: same id, new content.
pub(crate) fn patch(state: &mut EditorState, id: &str, fields: Value) {
    assert!(
        state.apply(EditorCommand::PatchNodeData {
            node_id: NodeId::new(id),
            patch_json: fields.to_string(),
            page_id: None,
        }),
        "patch {id}"
    );
}

/// What an agent's `I(parent, node)` does.
/// Returns the id the editor gave the new node (it allocates its own).
pub(crate) fn insert(state: &mut EditorState, parent: &str, node: Value) -> String {
    let node: PenNode = serde_json::from_value(node).expect("inserted node");
    let ids = state
        .insert_subtree_returning_root_ids(vec![node], &NodeId::new(parent))
        .unwrap_or_else(|| panic!("insert under {parent}"));
    ids.into_iter().next().expect("one inserted root")
}

/// One node's JSON without its children, or `Null` when it does not exist.
pub(crate) fn own(state: &EditorState, id: &str) -> Value {
    let found = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new(id));
    let mut value = found
        .and_then(|node| serde_json::to_value(node).ok())
        .unwrap_or(Value::Null);
    if let Some(fields) = value.as_object_mut() {
        fields.remove("children");
    }
    value
}

pub(crate) fn canonical(state: &EditorState) -> Value {
    serde_json::to_value(&state.doc).expect("canonical document")
}
