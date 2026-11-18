//! Provenance of a stock image is recorded when its result lands on a node.
//!
//! The record lives in `EditorState::image_provenance` (in memory, follows
//! the document across tab switches). Persisting it into the saved `.op`
//! needs a field in the jian schema, so the last test pins that known gap:
//! when the schema field exists, flip it into a round-trip assertion.

use op_editor_core::{DocumentScope, EditorState, NodeId};
use op_image_enrich::{
    apply_result_with_provenance, ImageProvenance, SEARCH_FAILED_PLACEHOLDER_SRC,
};
use serde_json::json;

const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

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

fn credit() -> ImageProvenance {
    ImageProvenance {
        provider: "wikimedia".into(),
        creator: Some("Jane Doe".into()),
        license: Some("CC BY 4.0".into()),
        license_url: Some("https://creativecommons.org/licenses/by/4.0/".into()),
        source_page_url: Some("https://commons.wikimedia.org/wiki/File:X.jpg".into()),
    }
}

#[test]
fn provenance_is_written_on_apply() {
    let mut state = state();
    let id = NodeId::new("slot");
    assert!(apply_result_with_provenance(
        &mut state,
        &id,
        PNG,
        Some(&credit())
    ));
    assert_eq!(state.image_provenance.get("slot"), Some(&credit()));

    // A new image without a record must not keep the previous credit.
    assert!(apply_result_with_provenance(
        &mut state,
        &id,
        "data:image/png;base64,iVBORw0KGgoAAAA=",
        None
    ));
    assert!(state.image_provenance.is_empty());
}

#[test]
fn a_refused_or_failed_write_records_nothing() {
    let mut state = state();
    let id = NodeId::new("slot");
    // The failure sentinel is not an image: no credit line for it.
    apply_result_with_provenance(
        &mut state,
        &id,
        SEARCH_FAILED_PLACEHOLDER_SRC,
        Some(&credit()),
    );
    assert!(state.image_provenance.is_empty());
    // A vanished node: nothing lands, nothing is recorded.
    assert!(!apply_result_with_provenance(
        &mut state,
        &NodeId::new("missing"),
        PNG,
        Some(&credit())
    ));
    assert!(state.image_provenance.is_empty());
}

#[test]
fn provenance_follows_its_document_across_a_tab_switch() {
    let mut state = state();
    apply_result_with_provenance(&mut state, &NodeId::new("slot"), PNG, Some(&credit()));
    let mut parked = DocumentScope::from_state(EditorState::new());
    state.exchange_document_scope(&mut parked);
    assert!(
        state.image_provenance.is_empty(),
        "the other document has no credits"
    );
    state.exchange_document_scope(&mut parked);
    assert_eq!(state.image_provenance.get("slot"), Some(&credit()));
}

#[test]
fn provenance_is_written_on_apply_and_save_load_persistence_is_the_known_gap() {
    let mut state = state();
    apply_result_with_provenance(&mut state, &NodeId::new("slot"), PNG, Some(&credit()));
    // TODO(jian schema): once `PenNodeBase` carries the record, assert that
    // it survives this round trip. Today the saved document has no trace.
    let saved = serde_json::to_string(&state.doc).unwrap();
    for key in op_editor_core::PROVENANCE_KEYS {
        assert!(
            !saved.contains(&format!("\"{key}\"")),
            "{key} unexpectedly persisted"
        );
    }
    let reloaded = EditorState::from_document(serde_json::from_str(&saved).unwrap());
    assert!(reloaded.image_provenance.is_empty());
}
