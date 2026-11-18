//! An applied Iconify icon is an SVG data URL. It must settle its slot like
//! any other image: not a target again (the desktop session would otherwise
//! re-search it on every rescan), not a failed or fallback slot.

use std::collections::HashSet;

use op_editor_core::{EditorState, NodeId};
use op_image_enrich::{
    apply_result_with_credit, collect_targets, is_failed_image_slot_for_host, is_image_fallback,
    ImageCredit,
};
use serde_json::json;

/// The shape `fetch_svg_data_url` produces for an Iconify icon: a base64 SVG
/// data URL (a 24-unit `currentColor` square at `height=256`).
const ICON_DATA_URL: &str =
    "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNTYiIGhlaWdodD0iMjU2IiB2aWV3Qm94PSIwIDAgMjQgMjQiPjxwYXRoIGZpbGw9ImN1cnJlbnRDb2xvciIgZD0iTTMgM2gxOHYxOEgzeiIvPjwvc3ZnPg==";

fn icon_data_url() -> String {
    ICON_DATA_URL.to_string()
}

fn slot_state(kind: &str) -> EditorState {
    let child = match kind {
        "image" => json!({
            "type": "image", "id": "slot", "name": "Icon", "width": 64, "height": 64,
            "src": "", "imageSearchQuery": "vector: shopping cart"
        }),
        _ => json!({
            "type": "frame", "id": "slot", "name": "Icon", "width": 64, "height": 64,
            "imageSearchQuery": "vector: shopping cart"
        }),
    };
    let document =
        serde_json::from_value(json!({"version": "1.0.0", "children": [child]})).expect("document");
    EditorState::from_document(document)
}

#[test]
fn an_applied_svg_icon_settles_its_slot() {
    for kind in ["image", "frame"] {
        let mut state = slot_state(kind);
        assert_eq!(
            collect_targets(&state, &HashSet::new()).len(),
            1,
            "{kind}: the empty slot is a target"
        );
        let credit = ImageCredit {
            provider: "iconify".into(),
            creator: Some("Pictogrammers".into()),
            license: Some("Apache 2.0".into()),
            license_url: None,
            source_page_url: None,
        };
        assert!(apply_result_with_credit(
            &mut state,
            &NodeId::new("slot"),
            &icon_data_url(),
            Some(&credit)
        ));
        assert!(
            collect_targets(&state, &HashSet::new()).is_empty(),
            "{kind}: an applied icon must not be searched again"
        );
        let node = state.active_children().first().expect("slot");
        assert!(!is_failed_image_slot_for_host(node), "{kind}");
        assert!(!is_image_fallback(node), "{kind}");
    }
}
