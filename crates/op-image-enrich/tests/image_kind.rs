//! A slot's kind (photo or vector) travels in its search text, because the
//! `.op` schema drops unknown node fields. The default stays photo.

use std::collections::HashSet;

use op_editor_core::EditorState;
use op_image_enrich::{collect_targets, ImageKind};
use serde_json::json;

fn state(query: &str) -> EditorState {
    let document = serde_json::from_value(json!({
        "version": "1.0.0", "children": [{
            "type": "image", "id": "slot", "name": "Slot",
            "width": 64, "height": 64, "src": "",
            "imageSearchQuery": query
        }]
    }))
    .unwrap();
    EditorState::from_document(document)
}

fn only_target(state: &EditorState) -> op_image_enrich::ImageSearchTarget {
    let mut targets = collect_targets(state, &HashSet::new());
    assert_eq!(targets.len(), 1, "{targets:?}");
    targets.remove(0)
}

#[test]
fn g_default_kind_is_photo_and_vector_is_opt_in() {
    let photo = only_target(&state("red bicycle on a street"));
    assert_eq!(photo.kind, ImageKind::Photo);
    assert_eq!(photo.query, "red bicycle on a street");

    let vector = only_target(&state("vector: shopping cart"));
    assert_eq!(vector.kind, ImageKind::Vector);
    assert_eq!(
        vector.query, "shopping cart",
        "providers never see the marker"
    );

    // The marker is case-insensitive and survives a save and reopen of the
    // document (it is part of the stored search text, not a dropped field).
    let reopened = {
        let written = serde_json::to_string(&state("Vector: home").doc).unwrap();
        EditorState::from_document(jian_ops_schema::load_str(&written).unwrap().value)
    };
    let again = only_target(&reopened);
    assert_eq!(
        (again.kind, again.query.as_str()),
        (ImageKind::Vector, "home")
    );

    // A slot whose search text is only the marker has no keywords: no target.
    assert!(collect_targets(&state("vector:"), &HashSet::new()).is_empty());
}
