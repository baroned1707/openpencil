//! Text over a backdrop that is not one solid colour is reported as
//! `contrastUncertain`, never as a pass or a fail guessed against the wrong
//! colour.

use super::*;
use serde_json::json;

fn state_for(tree: serde_json::Value, variables: Option<serde_json::Value>) -> EditorState {
    let mut document = json!({ "version": "1.0", "children": [tree] });
    if let Some(variables) = variables {
        document["variables"] = variables;
    }
    EditorState::from_document(serde_json::from_value(document).expect("document"))
}

fn scan(tree: serde_json::Value, variables: Option<serde_json::Value>) -> ContrastScan {
    let state = state_for(tree, variables);
    let theme = op_editor_core::variables_resolve::effective_theme(
        &state.doc,
        &state.ui.variables.active_theme,
    );
    scan_contrast(
        state.active_children(),
        state.doc.variables.as_ref(),
        &theme,
    )
}

fn label(id: &str, color: &str) -> serde_json::Value {
    json!({ "type": "text", "id": id, "content": "Welcome back", "width": 120, "height": 20,
            "fill": [{ "type": "solid", "color": color }] })
}

fn frame_with(fill: serde_json::Value, children: Vec<serde_json::Value>) -> serde_json::Value {
    json!({ "type": "frame", "id": "root", "width": 300, "height": 200,
            "fill": fill, "children": children })
}

fn image() -> serde_json::Value {
    json!({ "type": "image", "url": "https://example.test/hero.jpg" })
}

fn assert_uncertain(scan: &ContrastScan, reason: &str) {
    assert!(
        scan.issues.is_empty(),
        "no pass/fail guess: {:?}",
        scan.issues
    );
    assert_eq!(scan.uncertain.len(), 1, "{:?}", scan.uncertain);
    let entry = &scan.uncertain[0];
    assert_eq!(entry.node_id, "t");
    assert_eq!(entry.text, "Welcome back");
    assert_eq!(entry.reason, reason);
    assert!(entry.advice.contains("get_screenshot"), "{}", entry.advice);
}

/// Invariant: over ONE solid colour the scan still passes or fails by ratio.
#[test]
fn text_over_a_solid_passes_or_fails_as_before() {
    let passing = scan(
        frame_with(
            json!([{ "type": "solid", "color": "#FFFFFF" }]),
            vec![label("t", "#111111")],
        ),
        None,
    );
    assert!(passing.issues.is_empty() && passing.uncertain.is_empty());

    let failing = scan(
        frame_with(
            json!([{ "type": "solid", "color": "#888888" }]),
            vec![label("t", "#777777")],
        ),
        None,
    );
    assert_eq!(failing.issues.len(), 1);
    assert!(failing.uncertain.is_empty());
}

#[test]
fn dark_theme_solid_that_fails_contrast_is_still_reported() {
    let dark = scan(
        frame_with(
            json!([{ "type": "solid", "color": "#0F172A" }]),
            vec![label("t", "#1E293B")],
        ),
        None,
    );
    assert_eq!(dark.issues.len(), 1, "{dark:?}");
    assert_eq!(dark.issues[0].bg, "#0F172A");
    assert!(dark.uncertain.is_empty());
}

#[test]
fn text_over_an_image_fill_is_uncertain() {
    let result = scan(
        frame_with(json!([image()]), vec![label("t", "#FFFFFF")]),
        None,
    );
    assert_uncertain(&result, "image");
}

#[test]
fn text_over_a_gradient_is_uncertain() {
    let gradient = json!([{ "type": "linear_gradient", "angle": 90, "stops": [
        { "offset": 0, "color": "#000000" }, { "offset": 1, "color": "#FFFFFF" }] }]);
    let result = scan(frame_with(gradient, vec![label("t", "#FFFFFF")]), None);
    assert_uncertain(&result, "gradient");
}

/// An image with a 40% black overlay in the same fill list is still an image.
#[test]
fn an_image_under_a_forty_percent_black_fill_is_uncertain_not_solid_black() {
    let fills = json!([image(), { "type": "solid", "color": "#000000", "opacity": 0.4 }]);
    let result = scan(frame_with(fills, vec![label("t", "#FFFFFF")]), None);
    assert_uncertain(&result, "image");
}

/// The overlay as its own frame on top of the image frame: translucent over an
/// unknown backdrop stays unknown.
#[test]
fn a_translucent_overlay_frame_over_an_image_is_uncertain() {
    let overlay = json!({ "type": "frame", "id": "scrim", "width": 300, "height": 200,
        "fill": [{ "type": "solid", "color": "#000000", "opacity": 0.4 }],
        "children": [label("t", "#FFFFFF")] });
    let result = scan(frame_with(json!([image()]), vec![overlay]), None);
    assert_uncertain(&result, "translucent");
}

/// Over a known solid the blend is exact, so the scan keeps judging it.
#[test]
fn a_translucent_fill_over_a_solid_parent_is_blended_and_judged() {
    let glass = json!({ "type": "frame", "id": "glass", "width": 100, "height": 50,
        "fill": [{ "type": "solid", "color": "#000000", "opacity": 0.4 }],
        "children": [label("t", "#555555")] });
    let result = scan(
        frame_with(
            json!([{ "type": "solid", "color": "#FFFFFF" }]),
            vec![glass],
        ),
        None,
    );
    assert!(result.uncertain.is_empty(), "{:?}", result.uncertain);
    assert_eq!(result.issues.len(), 1, "{:?}", result.issues);
    assert_eq!(result.issues[0].bg, "#999999");
}

#[test]
fn a_backdrop_variable_that_does_not_resolve_is_uncertain() {
    let result = scan(
        frame_with(
            json!([{ "type": "solid", "color": "$--missing" }]),
            vec![label("t", "#FFFFFF")],
        ),
        None,
    );
    assert_uncertain(&result, "unresolved");
}

#[test]
fn a_transparent_fill_changes_nothing_and_icons_are_never_uncertain() {
    let icon = json!({ "type": "icon_font", "id": "i", "iconFontName": "home",
                       "width": 20, "height": 20 });
    let result = scan(frame_with(json!([image()]), vec![icon]), None);
    assert!(result.issues.is_empty() && result.uncertain.is_empty());

    let clear = frame_with(
        json!([{ "type": "solid", "color": "transparent" }]),
        vec![label("t", "#111111")],
    );
    let result = scan(clear, None);
    assert!(result.issues.is_empty() && result.uncertain.is_empty());
}

#[test]
fn a_solid_card_on_an_image_is_judged_normally() {
    let card = json!({ "type": "frame", "id": "card", "width": 100, "height": 50,
        "fill": [{ "type": "solid", "color": "#FFFFFF" }],
        "children": [label("t", "#111111")] });
    let result = scan(frame_with(json!([image()]), vec![card]), None);
    assert!(result.issues.is_empty() && result.uncertain.is_empty());
}

/// The write result carries the new key only when there is something to say,
/// as short lines for an external agent and objects for the in-app loop.
#[test]
fn write_feedback_carries_contrast_uncertain_only_when_non_empty() {
    let clean = state_for(
        frame_with(
            json!([{ "type": "solid", "color": "#FFFFFF" }]),
            vec![label("t", "#111111")],
        ),
        None,
    );
    let clean_map =
        post_write_diagnostics(&clean, PostWriteExtras::default(), Some(&MCP_FEEDBACK_CAPS));
    assert!(
        !clean_map.contains_key("contrastUncertain"),
        "{clean_map:?}"
    );

    let on_image = state_for(
        frame_with(json!([image()]), vec![label("t", "#FFFFFF")]),
        None,
    );
    let external = post_write_diagnostics(
        &on_image,
        PostWriteExtras::default(),
        Some(&MCP_FEEDBACK_CAPS),
    );
    let lines = external["contrastUncertain"].as_array().expect("lines");
    let line = lines[0].as_str().expect("one short line");
    assert!(
        line.contains("node t") && line.contains("image") && line.contains("get_screenshot"),
        "{line}"
    );
    assert!(!external.contains_key("contrastIssues"), "{external:?}");

    let in_app = post_write_diagnostics(
        &on_image,
        PostWriteExtras::default(),
        Some(&IN_APP_FEEDBACK_CAPS),
    );
    assert_eq!(in_app["contrastUncertain"][0]["reason"], "image");
    assert_eq!(in_app["contrastUncertain"][0]["nodeId"], "t");

    // The public quality report lists it too, and stays free of the key when clean.
    let report = serde_json::to_value(collect_design_quality(&on_image)).expect("json");
    assert_eq!(report["contrastUncertain"][0]["text"], "Welcome back");
    let clean_report = serde_json::to_value(collect_design_quality(&clean)).expect("json");
    assert!(clean_report.get("contrastUncertain").is_none());
}
