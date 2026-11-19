//! Invariants of [`design_eval_metrics`]: every page is walked, the issue
//! counts come from the same collectors a design write uses, and the output is
//! deterministic.

use serde_json::{json, Value};

use super::*;
use crate::design_agent_tools::with_font_catalog_file;

fn state_of(document: Value) -> EditorState {
    EditorState::from_document(serde_json::from_value(document).expect("document"))
}

fn one_page(children: Vec<Value>) -> EditorState {
    state_of(json!({ "version": "1.0", "children": children }))
}

fn text(id: &str, content: &str, color: &str) -> Value {
    json!({ "type": "text", "id": id, "content": content, "width": 160, "height": 20,
            "fontSize": 14, "fill": [{ "type": "solid", "color": color }] })
}

/// A frame holding one icon whose glyph is a font family name (an icon issue).
fn broken_icon_screen(id: &str) -> Value {
    json!({ "type": "frame", "id": id, "name": "Screen", "width": 390, "height": 844,
            "children": [{ "type": "icon_font", "id": format!("{id}-icon"),
                           "iconFontName": "lucide", "width": 24, "height": 24 }] })
}

/// A frame with grey text on a slightly lighter grey (a contrast issue).
fn low_contrast_screen(id: &str) -> Value {
    json!({ "type": "frame", "id": id, "name": "Screen", "width": 390, "height": 844,
            "fill": [{ "type": "solid", "color": "#888888" }],
            "children": [text(&format!("{id}-t"), "Welcome back", "#777777")] })
}

fn metrics(state: &EditorState) -> DesignEvalMetrics {
    design_eval_metrics(state)
}

/// Invariant: the metrics walk EVERY page, not just the active one.
#[test]
fn issues_on_every_page_are_counted() {
    let mut state = state_of(json!({
        "version": "1.0",
        "children": [],
        "pages": [
            { "id": "p1", "name": "One", "children": [broken_icon_screen("a")] },
            { "id": "p2", "name": "Two", "children": [broken_icon_screen("b"), low_contrast_screen("c")] },
        ],
    }));
    state.ui.active_page_index = 0;
    let found = metrics(&state);
    assert_eq!(found.page_count, 2);
    assert_eq!(
        found.icon_issues, 2,
        "one icon issue on each page: {found:?}"
    );
    assert_eq!(
        found.contrast_issues, 1,
        "page two's contrast issue: {found:?}"
    );
    assert_eq!(found.node_count, 6, "{found:?}");
    assert_eq!(
        state.ui.active_page_index, 0,
        "the input state is untouched"
    );
}

/// Invariant: on a one-page document every issue count equals the length of the
/// same key in `post_write_diagnostics` (no second implementation of a scan).
#[test]
fn issue_counts_equal_post_write_diagnostics_lengths() {
    let state = one_page(vec![
        broken_icon_screen("a"),
        low_contrast_screen("b"),
        json!({ "type": "frame", "id": "img", "name": "Hero", "width": 390, "height": 300,
                "fill": [{ "type": "image", "url": "https://example.test/p.jpg" }],
                "children": [text("over-image", "Over a photo", "#FFFFFF")] }),
        json!({ "type": "text", "id": "odd-font", "content": "Hi", "width": 80, "height": 20,
                "fontFamily": "Zzzz Not A Font" }),
    ]);
    // The same pinned font environment the metrics score under.
    let mut pinned = state.clone();
    pin_eval_font_environment(&mut pinned);
    let report = with_font_catalog_file(None, || {
        post_write_diagnostics(&pinned, PostWriteExtras::default(), None)
    });
    let len = |key: &str| {
        report
            .get(key)
            .and_then(Value::as_array)
            .map_or(0, Vec::len) as u32
    };
    let found = metrics(&state);
    assert_eq!(found.layout_issues, len("layoutIssues"));
    assert_eq!(found.icon_issues, len("iconIssues"));
    assert_eq!(found.contrast_issues, len("contrastIssues"));
    assert_eq!(found.contrast_uncertain, len("contrastUncertain"));
    assert_eq!(found.font_issues, len("fontIssues"));
    assert_eq!(found.structure_issues, len("structureIssues"));
    assert_eq!(found.nav_issues, len("navIssues"));
    // The fixture really exercises the keys it claims to compare.
    assert!(
        found.icon_issues >= 1 && found.contrast_issues >= 1,
        "{found:?}"
    );
    assert!(
        found.contrast_uncertain >= 1 && found.font_issues >= 1,
        "{found:?}"
    );
}

/// Invariant: the output is a pure function of the document.
#[test]
fn metrics_are_deterministic_and_serialize_stably() {
    let state = one_page(vec![broken_icon_screen("a"), low_contrast_screen("b")]);
    let first = serde_json::to_string(&metrics(&state)).unwrap();
    let second = serde_json::to_string(&metrics(&state)).unwrap();
    assert_eq!(first, second);
    assert!(
        first.starts_with("{\"layoutIssues\":") && first.contains(",\"iconIssues\":"),
        "field order is the declaration order: {first}"
    );
}

/// Invariant: `lorem ipsum` matches anywhere, the short words only as the whole
/// text, and a text input's placeholder property is never counted.
#[test]
fn placeholder_text_rules() {
    for filler in [
        "Lorem ipsum dolor sit amet",
        "  LOREM IPSUM ",
        "Title",
        " description ",
        "Text here",
    ] {
        assert!(is_placeholder_text(filler), "{filler:?}");
    }
    for real in [
        "Order title",
        "Description of your order",
        "Titled",
        "Sign up",
    ] {
        assert!(!is_placeholder_text(real), "{real:?}");
    }
    let state = one_page(vec![json!({
        "type": "frame", "id": "f", "width": 300, "height": 200, "children": [
            text("a", "Lorem ipsum dolor", "#111111"),
            text("b", "Title", "#111111"),
            text("c", "Real headline", "#111111"),
            { "type": "text_input", "id": "i", "placeholder": "Title", "width": 200, "height": 40 },
        ]
    })]);
    assert_eq!(metrics(&state).placeholder_text, 2);
}

/// Invariant: a run of 3+ like siblings with the same text in one position is
/// counted as duplicate rows; distinct rows keep the coverage at 1000.
#[test]
fn repeated_rows_with_the_same_text_lower_coverage() {
    let row = |id: &str, name: &str| {
        json!({ "type": "frame", "id": id, "name": "Row", "width": 300, "height": 40,
                "children": [text(&format!("{id}-n"), name, "#111111"),
                             text(&format!("{id}-s"), "Active", "#111111")] })
    };
    let list = |rows: Vec<Value>| {
        one_page(vec![
            json!({ "type": "frame", "id": "list", "width": 300, "height": 400,
                              "children": rows }),
        ])
    };
    let found = metrics(&list(vec![
        row("r1", "Ada"),
        row("r2", "Grace"),
        row("r3", "Linus"),
    ]));
    // 3 names (all distinct) + 3 statuses (all "Active"): 2 duplicates of 6 rows.
    assert_eq!((found.repeated_row_texts, found.duplicate_row_text), (6, 2));
    assert_eq!(found.content_coverage_permille, 666);
    let copies = metrics(&list(vec![
        row("r1", "Ada"),
        row("r2", "Ada"),
        row("r3", "Ada"),
    ]));
    assert_eq!(copies.duplicate_row_text, 4);
    assert!(copies.content_coverage_permille < found.content_coverage_permille);
    let pair = metrics(&list(vec![row("r1", "Ada"), row("r2", "Ada")]));
    assert_eq!(
        (pair.repeated_row_texts, pair.content_coverage_permille),
        (0, 1000)
    );
}

#[test]
fn token_conformance_counts_variable_bound_solid_fills() {
    let state = one_page(vec![json!({
        "type": "frame", "id": "f", "width": 300, "height": 200,
        "fill": [{ "type": "solid", "color": "$surface" }],
        "children": [text("a", "Bound", "$foreground"), text("b", "Raw", "#111111")]
    })]);
    let found = metrics(&state);
    assert_eq!((found.token_bound_fills, found.solid_fills), (2, 3));
    assert_eq!(found.token_conformance_permille, 666);
}

#[test]
fn small_interactive_nodes_and_font_sizes_are_reported() {
    let state = one_page(vec![json!({
        "type": "frame", "id": "screen", "name": "Screen", "width": 390, "height": 400, "children": [
            { "type": "frame", "id": "sec1", "name": "A", "width": 390, "height": 100, "children": [
                text("t1", "Small", "#111111"),
                { "type": "text", "id": "t2", "content": "Big", "fontSize": 28, "width": 100, "height": 30 },
                { "type": "frame", "id": "tiny", "name": "Close Button", "width": 16, "height": 16, "children": [] },
                { "type": "frame", "id": "ok", "name": "Submit button", "width": 120, "height": 44, "children": [] },
                { "type": "frame", "id": "tbl", "name": "Table", "width": 120, "height": 10, "children": [] },
            ] },
        ]
    })]);
    let found = metrics(&state);
    assert_eq!(found.interactive_nodes, 2, "{found:?}");
    assert_eq!(found.tap_targets_under_24, 1, "{found:?}");
    assert_eq!(found.type_sizes_max_per_section, 2, "{found:?}");
}
