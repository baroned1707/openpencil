//! Tests for the post-write feedback: glyph-name validation, default-ink text
//! contrast and the capped output external MCP callers receive. A child module
//! of `design_agent_tools`, so `super::` resolves its private scans.

use std::collections::BTreeMap;

use serde_json::json;

use super::*;

fn nodes(value: serde_json::Value) -> Vec<PenNode> {
    serde_json::from_value(value).expect("nodes")
}

fn icon_issues_for(name: &str, family: Option<&str>) -> Vec<String> {
    let mut icon = json!({ "type": "icon_font", "id": "probe", "iconFontName": name,
                           "width": 20, "height": 20 });
    if let Some(family) = family {
        icon["iconFontFamily"] = json!(family);
    }
    scan_icon_issues(&nodes(json!([
        { "type": "frame", "id": "root", "width": 100, "height": 100, "children": [icon] }
    ])))
}

/// Invariant: a glyph name the catalog does not know yields one line that names
/// the node, the bad name and up to three nearest glyphs, the same every time.
#[test]
fn unknown_icon_name_reports_nearest_glyphs() {
    let issues = icon_issues_for("serch", None);
    assert_eq!(issues.len(), 1, "{issues:?}");
    let line = &issues[0];
    assert!(
        line.starts_with("icon probe: 'serch' is not a lucide glyph; closest: search"),
        "{line}"
    );
    let closest = line.split("closest: ").nth(1).expect("closest list");
    assert!(closest.split(", ").count() <= 3, "{line}");
    assert_eq!(
        icon_issues_for("serch", None),
        issues,
        "must be deterministic"
    );
    // A whole hyphen segment counts as near: `train` -> `train-front`.
    assert!(icon_issues_for("train", Some("lucide"))[0].contains("train-front"));
    // Feather names its own family in the line.
    assert!(icon_issues_for("serch", Some("feather"))[0].contains("is not a feather glyph"));
}

/// Invariant: a legacy lucide name the painter still resolves (alias table,
/// feather fallback) is never flagged, or valid designs would be "fixed" into
/// worse ones.
#[test]
fn legacy_icon_alias_is_not_flagged() {
    use op_editor_ui::widgets::icon_catalog::lookup_icon;
    for legacy in ["more-horizontal", "check-circle", "edit", "grid", "sliders"] {
        assert!(
            lookup_icon("lucide", legacy).is_some(),
            "precondition: the painter resolves legacy name {legacy}"
        );
        assert!(
            icon_issues_for(legacy, Some("lucide")).is_empty(),
            "{legacy} paints, so it must not be flagged"
        );
    }
}

/// Invariant: real glyphs, unchecked families and a missing family (lucide)
/// produce no icon issue.
#[test]
fn known_glyph_is_not_flagged() {
    for name in ["home", "search", "compass", "shopping-cart"] {
        assert!(icon_issues_for(name, None).is_empty(), "{name}");
        assert!(icon_issues_for(name, Some("lucide")).is_empty(), "{name}");
    }
    assert!(icon_issues_for("activity", Some("feather")).is_empty());
    // A brand set is runtime-loaded and a foreign family unknown: both skipped.
    assert!(icon_issues_for("no-such-brand-logo", Some("simple-icons")).is_empty());
    assert!(icon_issues_for("whatever", Some("Material Symbols Rounded")).is_empty());
}

/// Invariant: the existing empty-name check still owns an empty name, so a
/// node is reported once, not twice.
#[test]
fn empty_icon_name_is_reported_once() {
    let issues = icon_issues_for("", None);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].contains("missing"), "{issues:?}");
}

fn contrast_for(
    tree: serde_json::Value,
    variables: Option<serde_json::Value>,
) -> Vec<ContrastIssue> {
    let mut document = json!({ "version": "1.0", "children": [tree] });
    if let Some(variables) = variables {
        document["variables"] = variables;
    }
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(document).expect("document");
    let state = EditorState::from_document(doc);
    let theme = op_editor_core::variables_resolve::effective_theme(
        &state.doc,
        &state.ui.variables.active_theme,
    );
    scan_contrast_issues(
        state.active_children(),
        state.doc.variables.as_ref(),
        &theme,
    )
}

fn text_on(background: &str, fill: Option<serde_json::Value>) -> serde_json::Value {
    let mut text = json!({ "type": "text", "id": "label", "content": "Hello",
                           "width": 80, "height": 20 });
    if let Some(fill) = fill {
        text["fill"] = fill;
    }
    json!({ "type": "frame", "id": "root", "width": 200, "height": 100,
            "fill": [{ "type": "solid", "color": background }], "children": [text] })
}

/// Invariant: text with no resolvable fill paints the default near-black ink,
/// so it is reported only where that ink is unreadable (a dark background) and
/// only when no fill resolves.
#[test]
fn invisible_text_is_flagged_only_without_a_resolvable_fill() {
    // No fill on a dark surface: flagged, against the default ink.
    let dark = contrast_for(text_on("#0F172A", None), None);
    assert_eq!(dark.len(), 1, "{dark:?}");
    assert_eq!(dark[0].node_id, "label");
    assert_eq!(dark[0].fg, "#141414");
    // No fill on a light surface: the default ink reads fine.
    assert!(contrast_for(text_on("#FFFFFF", None), None).is_empty());
    // An explicit, readable fill on the same dark surface: nothing to say.
    let readable = json!([{ "type": "solid", "color": "#FFFFFF" }]);
    assert!(contrast_for(text_on("#0F172A", Some(readable)), None).is_empty());
    // A variable fill that resolves is a fill: nothing to say.
    let variables = json!({ "ink": { "type": "color", "value": "#FFFFFF" } });
    let resolvable = json!([{ "type": "solid", "color": "$ink" }]);
    assert!(contrast_for(text_on("#0F172A", Some(resolvable)), Some(variables)).is_empty());
    // A variable that resolves to nothing paints the default ink: flagged.
    let dangling = json!([{ "type": "solid", "color": "$missing" }]);
    assert_eq!(
        contrast_for(text_on("#0F172A", Some(dangling)), None).len(),
        1
    );
    // A gradient is a deliberate paint the scan cannot judge: left alone.
    let gradient = json!([{ "type": "linear_gradient", "stops": [
        { "offset": 0, "color": "#FFFFFF" }, { "offset": 1, "color": "#CCCCCC" }] }]);
    assert!(contrast_for(text_on("#0F172A", Some(gradient)), None).is_empty());
}

/// A dark card holding `count` texts with no fill (each one a contrast issue),
/// plus a few misspelled icons.
fn state_with_many_findings(count: usize) -> EditorState {
    let mut children: Vec<serde_json::Value> = (0..count)
        .map(|index| {
            json!({ "type": "text", "id": format!("text{index}"), "content": "Label",
                    "width": 80, "height": 20 })
        })
        .collect();
    children.extend((0..4).map(|index| {
        json!({ "type": "icon_font", "id": format!("icon{index}"),
                "iconFontName": format!("not-a-real-glyph-{index}"), "width": 20, "height": 20 })
    }));
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version": "1.0",
        "children": [{ "type": "frame", "id": "root", "width": 400, "height": 400,
                       "layout": "none",
                       "fill": [{ "type": "solid", "color": "#0F172A" }],
                       "children": children }]
    }))
    .expect("document");
    EditorState::from_document(doc)
}

/// Invariant: external feedback holds at most 12 items per key and the whole
/// appendix fits the byte budget, and says so when it cut something; the
/// in-app call (no caps) is not shortened.
#[test]
fn appended_diagnostics_are_capped() {
    let state = state_with_many_findings(40);
    let capped =
        post_write_diagnostics(&state, PostWriteExtras::default(), Some(&MCP_FEEDBACK_CAPS));
    let contrast_lines = capped["contrastIssues"].as_array().expect("contrastIssues");
    assert_eq!(contrast_lines.len(), MCP_FEEDBACK_CAPS.max_items);
    assert!(contrast_lines.iter().all(serde_json::Value::is_string));
    let serialized = serde_json::Value::Object(capped.clone()).to_string();
    assert!(
        serialized.len() <= MCP_FEEDBACK_CAPS.max_total_bytes,
        "{}",
        serialized.len()
    );
    assert_eq!(capped["diagnosticsTruncated"], json!(true));

    let uncapped = post_write_diagnostics(&state, PostWriteExtras::default(), None);
    assert_eq!(
        uncapped["contrastIssues"]
            .as_array()
            .expect("contrast")
            .len(),
        40
    );
    assert!(uncapped["contrastIssues"][0].is_object());
    assert!(!uncapped.contains_key("diagnosticsTruncated"));

    // A byte budget tighter than the item cap drops hints first, then items.
    let tight = FeedbackCaps {
        max_total_bytes: 600,
        ..MCP_FEEDBACK_CAPS
    };
    let squeezed = post_write_diagnostics(&state, PostWriteExtras::default(), Some(&tight));
    assert!(
        serde_json::Value::Object(squeezed.clone())
            .to_string()
            .len()
            <= 600
    );
    assert!(!squeezed.contains_key("layoutHint"));
}

/// Invariant: a long line is cut on a char boundary (the messages contain
/// multi-byte characters) and marked, never split mid-character.
#[test]
fn long_lines_are_cut_on_a_char_boundary() {
    let caps = FeedbackCaps {
        max_items: 2,
        max_item_chars: 20,
        max_total_bytes: 4096,
    };
    let mut lines = vec![
        "\u{2014}".repeat(50),
        "short".to_string(),
        "dropped".to_string(),
    ];
    assert!(cap_lines(&mut lines, &caps));
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].chars().count(), 20);
    assert!(lines[0].ends_with("..."));
    assert_eq!(lines[1], "short");
}

/// Invariant: a clean page yields no keys at all (so a clean write result is
/// byte-identical to the result before this feature existed).
#[test]
fn clean_page_yields_no_feedback_keys() {
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version": "1.0",
        "children": [{ "type": "frame", "id": "root", "width": 200, "height": 100,
                       "layout": "none",
                       "fill": [{ "type": "solid", "color": "#FFFFFF" }], "children": [
            { "type": "text", "id": "t", "content": "Hi", "width": 80, "height": 20,
              "fill": [{ "type": "solid", "color": "#111111" }] },
            { "type": "icon_font", "id": "i", "iconFontName": "home", "width": 20, "height": 20 }
        ] }]
    }))
    .expect("document");
    let state = EditorState::from_document(doc);
    let feedback =
        post_write_diagnostics(&state, PostWriteExtras::default(), Some(&MCP_FEEDBACK_CAPS));
    assert!(feedback.is_empty(), "{feedback:?}");
}

/// Invariant: the extras only the in-app wrapper knows (repair notes, the
/// root-seed hint) ride the same function and reach its keys.
#[test]
fn in_app_extras_reach_structure_issues_and_the_hint() {
    let state = EditorState::from_document(
        serde_json::from_value(json!({ "version": "1.0", "children": [] })).expect("document"),
    );
    let extras = PostWriteExtras {
        structure_notes: vec!["removed 1 extra status bar(s)".to_string()],
        root_seed_hint: Some("seed hint".to_string()),
    };
    let feedback = post_write_diagnostics(&state, extras, None);
    assert_eq!(
        feedback["structureIssues"],
        json!(["removed 1 extra status bar(s)"])
    );
    assert_eq!(feedback["layoutHint"], json!("seed hint"));
    let _: BTreeMap<String, serde_json::Value> = feedback.into_iter().collect();
}
