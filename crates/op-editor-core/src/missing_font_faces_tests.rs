use super::*;

fn state(children: serde_json::Value) -> EditorState {
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(serde_json::json!({
        "version": "1.0.0",
        "children": children,
    }))
    .expect("document");
    EditorState::from_document(doc)
}

fn text(id: &str, family: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut node = serde_json::json!({
        "type": "text", "id": id, "content": "hi", "fontFamily": family,
    });
    for (key, value) in extra.as_object().unwrap() {
        node[key] = value.clone();
    }
    node
}

#[test]
fn weights_and_italics_are_collected_per_family() {
    let frame = serde_json::json!({
        "type": "frame", "id": "f",
        "children": [text("e", "Pacifico", serde_json::json!({"fontWeight": "bold"}))],
    });
    let state = state(serde_json::json!([
        text("a", "Pacifico", serde_json::json!({})),
        text("b", "pacifico", serde_json::json!({"fontWeight": "700"})),
        text(
            "c",
            "\"Pacifico\", cursive",
            serde_json::json!({"fontWeight": 640, "fontStyle": "italic"})
        ),
        text("d", "Other", serde_json::json!({"fontWeight": 900})),
        frame,
    ]));
    let faces = document_font_faces(&state, "Pacifico");
    assert_eq!(
        faces.into_iter().collect::<Vec<_>>(),
        vec![(400, false), (600, true), (700, false)]
    );
    assert_eq!(
        document_font_faces(&state, "Other")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![(900, false)]
    );
}

#[test]
fn styled_segments_and_instance_overrides_count() {
    let state = state(serde_json::json!([
        {
            "type": "text", "id": "s", "fontFamily": "Base",
            "content": [
                {"text": "a", "fontFamily": "Pacifico", "fontWeight": 300},
                {"text": "b", "fontWeight": 800},
            ],
        },
        {
            "type": "ref", "id": "r", "ref": "c",
            "descendants": {"label": {"fontFamily": "Pacifico", "fontWeight": "500", "fontStyle": "italic"}},
        },
    ]));
    assert_eq!(
        document_font_faces(&state, "Pacifico")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![(300, false), (500, true)]
    );
    assert_eq!(
        document_font_faces(&state, "Base")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![(800, false)]
    );
}

#[test]
fn an_unread_family_counts_as_regular() {
    let state = state(serde_json::json!([]));
    assert_eq!(
        document_font_faces(&state, "Nothing")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![(400, false)]
    );
}

#[test]
fn weights_snap_to_the_nearest_hundred_within_range() {
    for (raw, expected) in [
        (1, 100),
        (149, 100),
        (150, 200),
        (399, 400),
        (650, 700),
        (950, 900),
        (5000, 900),
    ] {
        assert_eq!(snap(raw), expected, "{raw}");
    }
}
