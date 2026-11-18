//! A credit describes one picture: replacing the picture by anything that is
//! not a provider result must clear it, in the same mutation.

use crate::host_image_panel_transitions::write_selected_image_src;
use crate::walkers::find_node;
use crate::{node_image_credit, EditorCommand, EditorState, ImageCredit, ImageFillMode, NodeId};
use serde_json::json;

const STOCK: &str = "data:image/png;base64,c3RvY2s=";
const LOCAL: &str = "data:image/png;base64,bG9jYWw=";

fn credit() -> ImageCredit {
    ImageCredit {
        provider: "openverse".into(),
        creator: Some("Jane Doe".into()),
        license: Some("CC BY 4.0".into()),
        license_url: None,
        source_page_url: None,
    }
}

/// An image node and a frame with an image fill, both carrying a credit.
fn credited_state() -> EditorState {
    let credit = serde_json::to_value(credit()).unwrap();
    let document = serde_json::from_value(json!({
        "version": "1.0.0", "children": [
            {"type": "image", "id": "photo", "width": 64, "height": 64,
             "src": STOCK, "imageCredit": credit},
            {"type": "frame", "id": "card", "width": 200, "height": 120,
             "fill": [{"type": "image", "url": STOCK}], "imageCredit": credit}
        ]
    }))
    .unwrap();
    EditorState::from_document(document)
}

fn credit_of(state: &EditorState, id: &str) -> Option<ImageCredit> {
    find_node(state.active_children(), &NodeId::new(id))
        .and_then(|node| node_image_credit(node).cloned())
}

#[test]
fn local_import_clears_a_stale_credit() {
    // The fill picker / file import path: `set_selected_fill_image_url`.
    for id in ["photo", "card"] {
        let mut state = credited_state();
        assert_eq!(credit_of(&state, id), Some(credit()), "fixture");
        state.set_single_selection(NodeId::new(id));
        assert!(state.set_selected_fill_image_url(LOCAL));
        assert_eq!(credit_of(&state, id), None, "{id}: file import");
    }
    // Drag-and-drop of an image file onto the node.
    for id in ["photo", "card"] {
        let mut state = credited_state();
        assert!(state.apply_image_drop(&NodeId::new(id), LOCAL, None));
        assert_eq!(credit_of(&state, id), None, "{id}: drop");
    }
    // A mode change keeps the same picture, so it keeps the credit.
    let mut state = credited_state();
    state.set_single_selection(NodeId::new("card"));
    assert!(state.set_selected_image_fill_mode(ImageFillMode::Crop));
    assert_eq!(credit_of(&state, "card"), Some(credit()));
}

#[test]
fn ai_generate_clears_a_stale_credit() {
    // Accepting a generated preview writes through `write_selected_image_src`
    // with no credit; picking a search hit passes that hit's credit.
    let mut state = credited_state();
    state.set_single_selection(NodeId::new("photo"));
    assert!(write_selected_image_src(&mut state, LOCAL, None));
    assert_eq!(credit_of(&state, "photo"), None);

    // The next search result sets a NEW credit.
    let fresh = ImageCredit {
        creator: Some("Someone Else".into()),
        ..credit()
    };
    assert!(write_selected_image_src(
        &mut state,
        STOCK,
        Some(fresh.clone())
    ));
    assert_eq!(credit_of(&state, "photo"), Some(fresh));

    // And it is one undo step: undo brings back the picture without a credit.
    assert!(state.undo());
    assert_eq!(credit_of(&state, "photo"), None);
}

#[test]
fn update_node_src_change_clears_a_stale_credit() {
    let patch = |state: &mut EditorState, id: &str, value: serde_json::Value| {
        state.apply(EditorCommand::PatchNodeData {
            node_id: NodeId::new(id),
            patch_json: value.to_string(),
            page_id: None,
        })
    };
    let mut state = credited_state();
    // Changing the src of an image node.
    assert!(patch(&mut state, "photo", json!({"src": LOCAL})));
    assert_eq!(credit_of(&state, "photo"), None);
    // Changing the url of an image fill.
    assert!(patch(
        &mut state,
        "card",
        json!({"fill": [{"type": "image", "url": LOCAL}]})
    ));
    assert_eq!(credit_of(&state, "card"), None);

    // An edit that leaves the picture alone keeps the credit.
    let mut state = credited_state();
    assert!(patch(&mut state, "photo", json!({"name": "Hero"})));
    assert!(patch(&mut state, "photo", json!({"src": STOCK})));
    assert_eq!(credit_of(&state, "photo"), Some(credit()));

    // A patch that states the credit itself (enrich_images) is not overridden.
    let fresh = ImageCredit {
        creator: Some("Someone Else".into()),
        ..credit()
    };
    assert!(patch(
        &mut state,
        "photo",
        json!({"src": LOCAL, "imageCredit": fresh})
    ));
    assert_eq!(credit_of(&state, "photo"), Some(fresh));
}
