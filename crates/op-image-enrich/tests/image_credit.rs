//! The credit of a stock image is part of the node that owns the picture.
//!
//! It is written by `apply_result_with_credit` in the same mutation as the url,
//! saved as `imageCredit` in the `.op` file, and follows the node through
//! copy, paste, duplicate, undo and a tab switch because nothing is stored
//! beside the node.

use op_editor_core::{
    node_image_credit, walkers, DocumentScope, EditorCommand, EditorState, NodeId, PenNodeExt as _,
};
use op_image_enrich::{
    apply_result, apply_result_with_credit, ImageCredit, SEARCH_FAILED_PLACEHOLDER_SRC,
};
use serde_json::json;

const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";
const OTHER_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAA=";

fn state() -> EditorState {
    let document = serde_json::from_value(json!({
        "version": "1.0.0", "children": [{
            "type": "image", "id": "slot", "name": "Hero",
            "width": 64, "height": 64, "src": "", "imageSearchQuery": "serum bottle"
        }]
    }))
    .unwrap();
    EditorState::from_document(document)
}

fn credit() -> ImageCredit {
    ImageCredit {
        provider: "wikimedia".into(),
        creator: Some("Jane Doe".into()),
        license: Some("CC BY 4.0".into()),
        license_url: Some("https://creativecommons.org/licenses/by/4.0/".into()),
        source_page_url: Some("https://commons.wikimedia.org/wiki/File:X.jpg".into()),
    }
}

fn credit_of(state: &EditorState, id: &str) -> Option<ImageCredit> {
    walkers::find_node(state.active_children(), &NodeId::new(id))
        .and_then(|node| node_image_credit(node).cloned())
}

fn src_of(state: &EditorState, id: &str) -> String {
    match walkers::find_node(state.active_children(), &NodeId::new(id)) {
        Some(jian_ops_schema::node::PenNode::Image(image)) => image.src.to_string(),
        _ => panic!("image node {id} expected"),
    }
}

#[test]
fn credit_is_written_on_apply_and_replaced_by_a_credit_less_result() {
    let mut state = state();
    let id = NodeId::new("slot");
    assert!(apply_result_with_credit(
        &mut state,
        &id,
        PNG,
        Some(&credit())
    ));
    assert_eq!(credit_of(&state, "slot"), Some(credit()));

    // A new image without a record must not keep the previous credit.
    assert!(apply_result(&mut state, &id, OTHER_PNG));
    assert_eq!(credit_of(&state, "slot"), None);
}

#[test]
fn a_refused_or_failed_write_records_nothing() {
    let mut state = state();
    let id = NodeId::new("slot");
    // The failure sentinel is not an image: no credit line for it.
    apply_result_with_credit(
        &mut state,
        &id,
        SEARCH_FAILED_PLACEHOLDER_SRC,
        Some(&credit()),
    );
    assert_eq!(credit_of(&state, "slot"), None);
    // A vanished node: nothing lands, nothing is recorded.
    assert!(!apply_result_with_credit(
        &mut state,
        &NodeId::new("missing"),
        PNG,
        Some(&credit())
    ));
}

#[test]
fn a_failed_search_clears_the_credit_of_the_picture_it_replaces() {
    let mut state = state();
    let id = NodeId::new("slot");
    apply_result_with_credit(&mut state, &id, PNG, Some(&credit()));
    // Re-searching the slot and failing swaps the photo for the placeholder.
    assert!(apply_result(&mut state, &id, SEARCH_FAILED_PLACEHOLDER_SRC));
    assert_eq!(credit_of(&state, "slot"), None);
}

#[test]
fn fill_credit_lives_on_the_node_that_owns_the_fill() {
    let document = serde_json::from_value(json!({
        "version": "1.0.0", "children": [
            {"type": "frame", "id": "card", "name": "Card", "width": 200, "height": 120,
             "children": [
                {"type": "text", "id": "caption", "content": "Hello"}
             ],
             "fill": [{"type": "image", "url": "", "mode": "fill"}]},
            {"type": "rectangle", "id": "tile", "name": "Tile", "width": 80, "height": 80,
             "fill": [{"type": "image", "url": ""}]}
        ]
    }))
    .unwrap();
    let mut state = EditorState::from_document(document);
    assert!(apply_result_with_credit(
        &mut state,
        &NodeId::new("card"),
        PNG,
        Some(&credit())
    ));
    assert!(apply_result_with_credit(
        &mut state,
        &NodeId::new("tile"),
        OTHER_PNG,
        Some(&credit())
    ));
    assert_eq!(credit_of(&state, "card"), Some(credit()));
    assert_eq!(credit_of(&state, "tile"), Some(credit()));
    // The credit is on the frame / rectangle, never on a descendant, and the
    // saved JSON carries it on the same object as the fill.
    let saved = serde_json::to_value(&state.doc).unwrap();
    let card = &saved["children"][0];
    assert_eq!(card["imageCredit"]["creator"], "Jane Doe");
    assert!(card["fill"][0].get("imageCredit").is_none());
    let tile = &saved["children"][1];
    assert_eq!(tile["imageCredit"]["provider"], "wikimedia");
}

#[test]
fn image_credit_round_trips_through_save_and_load() {
    let mut state = state();
    apply_result_with_credit(&mut state, &NodeId::new("slot"), PNG, Some(&credit()));
    let saved = serde_json::to_string(&state.doc).unwrap();
    assert!(saved.contains("\"imageCredit\""));
    assert!(saved.contains("\"licenseUrl\""));
    assert!(saved.contains("\"sourcePageUrl\""));
    let reloaded = EditorState::from_document(serde_json::from_str(&saved).unwrap());
    assert_eq!(credit_of(&reloaded, "slot"), Some(credit()));
}

#[test]
fn old_file_without_credit_loads() {
    let document: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version": "1.0.0", "children": [
            {"type": "image", "id": "old", "width": 10, "height": 10, "src": "https://x/y.png"}
        ]
    }))
    .unwrap();
    let state = EditorState::from_document(document);
    assert_eq!(credit_of(&state, "old"), None);
    // And a document with no credits serialises without the key at all.
    assert!(!serde_json::to_string(&state.doc)
        .unwrap()
        .contains("imageCredit"));
}

#[test]
fn copy_and_duplicate_keep_the_credit() {
    let mut state = state();
    let id = NodeId::new("slot");
    apply_result_with_credit(&mut state, &id, PNG, Some(&credit()));

    // CopyNode command (what the MCP `C(...)` operation lowers to).
    assert!(state.apply(EditorCommand::CopyNode {
        node_id: id.clone(),
        target_parent: NodeId::NONE,
        overrides_json: None,
        page_id: None,
    }));
    // Duplicate (Cmd+D) and clipboard copy / paste.
    state.set_single_selection(id.clone());
    let mut next_id = 1000;
    let duplicate = state
        .duplicate_selected(&mut next_id, 10.0)
        .expect("duplicate");
    state.set_single_selection(id.clone());
    assert!(state.copy_selected());
    let pasted = state.paste_clipboard(&mut next_id, 20.0);
    assert_eq!(pasted.len(), 1);

    let copies: Vec<String> = state
        .active_children()
        .iter()
        .map(|node| node.id_str().to_string())
        .filter(|node_id| node_id != "slot")
        .collect();
    assert_eq!(
        copies.len(),
        3,
        "copy, duplicate and paste each added a node"
    );
    for copy in copies {
        assert_eq!(credit_of(&state, &copy), Some(credit()), "{copy}");
    }
    assert!(credit_of(&state, duplicate.as_str()).is_some());
}

#[test]
fn a_copy_whose_override_swaps_the_picture_drops_the_credit() {
    let mut state = state();
    let id = NodeId::new("slot");
    apply_result_with_credit(&mut state, &id, PNG, Some(&credit()));
    let before = state.active_children().len();
    assert!(state.apply(EditorCommand::CopyNode {
        node_id: id,
        target_parent: NodeId::NONE,
        overrides_json: Some(json!({"src": OTHER_PNG}).to_string()),
        page_id: None,
    }));
    let copy = state.active_children()[before].id_str().to_string();
    assert_eq!(src_of(&state, &copy), OTHER_PNG);
    assert_eq!(credit_of(&state, &copy), None);
}

#[test]
fn undo_removes_image_and_credit_together() {
    let mut state = state();
    let id = NodeId::new("slot");
    // The live-document path of `enrich_images`: one PatchNodeData carrying
    // the url and the credit, applied through the normal command pump.
    let patch = json!({"src": PNG, "imageCredit": credit()}).to_string();
    assert!(state.apply(EditorCommand::Batch {
        commands: vec![EditorCommand::PatchNodeData {
            node_id: id,
            patch_json: patch,
            page_id: None,
        }],
    }));
    assert_eq!(src_of(&state, "slot"), PNG);
    assert_eq!(credit_of(&state, "slot"), Some(credit()));

    assert!(state.undo());
    assert_eq!(src_of(&state, "slot"), "");
    assert_eq!(credit_of(&state, "slot"), None);

    assert!(state.redo());
    assert_eq!(src_of(&state, "slot"), PNG);
    assert_eq!(credit_of(&state, "slot"), Some(credit()));
}

#[test]
fn credit_follows_its_document_across_a_tab_switch() {
    let mut state = state();
    apply_result_with_credit(&mut state, &NodeId::new("slot"), PNG, Some(&credit()));
    let mut parked = DocumentScope::from_state(EditorState::new());
    state.exchange_document_scope(&mut parked);
    assert_eq!(
        credit_of(&state, "slot"),
        None,
        "the other document has no credits"
    );
    state.exchange_document_scope(&mut parked);
    assert_eq!(credit_of(&state, "slot"), Some(credit()));
}
