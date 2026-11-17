//! Tests for the font-family feedback (`fontIssues`). A child module of
//! `design_agent_tools`, so `super::` resolves its private scans.

use std::path::PathBuf;
use std::sync::Arc;

use op_font_fetch::Catalog;
use serde_json::json;

use super::font_catalog_file::CatalogFileCache;
use super::font_issues::{scan_font_issues, FontKnowledge};
use super::*;

const CLEAN_FAMILY_NODE: &str =
    r##"{"type":"text","id":"t0","content":"Hi","fontFamily":"Inter"}"##;

fn page_of(families: &[&str]) -> Vec<PenNode> {
    let texts: Vec<serde_json::Value> = families
        .iter()
        .enumerate()
        .map(|(index, family)| {
            json!({ "type": "text", "id": format!("t{index}"), "content": "Hello",
                    "fontFamily": family })
        })
        .collect();
    serde_json::from_value(json!([
        { "type": "frame", "id": "root", "width": 200, "height": 200, "children": texts }
    ]))
    .expect("nodes")
}

fn catalog_of(entries: &[(&str, &str, &str)]) -> Arc<Catalog> {
    let rows: Vec<serde_json::Value> = entries
        .iter()
        .map(|(id, family, license)| {
            json!({ "id": id, "family": family, "license": license, "type": "google" })
        })
        .collect();
    Arc::new(Catalog::parse(json!(rows).to_string().as_bytes()).expect("catalog"))
}

fn sample_catalog() -> Arc<Catalog> {
    catalog_of(&[
        ("poppins", "Poppins", "OFL-1.1"),
        ("sora", "Sora", "OFL-1.1"),
        ("lobster", "Lobster", "OFL-1.1"),
        ("proprietary", "Proprietary Sans", "Commercial"),
    ])
}

/// A live editor: system fonts enumerated, `Helvetica Neue` among them.
fn live(catalog: Option<Arc<Catalog>>) -> FontKnowledge {
    FontKnowledge::new(["Helvetica Neue", "Menlo"], true, catalog)
}

/// A headless `--mcp` process: system fonts were never enumerated.
fn headless(catalog: Option<Arc<Catalog>>) -> FontKnowledge {
    FontKnowledge::new([], false, catalog)
}

fn scratch_file(name: &str, content: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("op-font-issues-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("fontsource-catalog.json");
    std::fs::write(&path, content).expect("write scratch");
    path
}

/// Invariant: a family in no list yields one line naming it, saying it is not
/// installed and not downloadable, and offering at most three nearest names,
/// the same every time.
#[test]
fn unknown_family_reports_closest_names() {
    let known = live(Some(sample_catalog()));
    let issues = scan_font_issues(&page_of(&["Poppin"]), &known);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(
        issues[0].starts_with("font 'Poppin': not installed and not in the Google Fonts catalog")
    );
    let closest = issues[0].split("closest: ").nth(1).expect("closest clause");
    let names: Vec<&str> = closest
        .split(" (")
        .next()
        .expect("names")
        .split(", ")
        .collect();
    assert!(names.len() <= 3, "{names:?}");
    assert_eq!(names[0], "Poppins", "{issues:?}");
    assert!(issues[0].ends_with("(the editor falls back to a default)"));
    assert_eq!(issues, scan_font_issues(&page_of(&["Poppin"]), &known));
}

/// Invariant: a family that is in the catalog (allowed licence) but not
/// installed gets the advisory download line, not an error; one whose licence
/// is not on the allow-list is told it will not be downloaded.
#[test]
fn catalog_family_not_installed_reports_download_hint() {
    let known = live(Some(sample_catalog()));
    let issues = scan_font_issues(&page_of(&["Poppins"]), &known);
    assert_eq!(
        issues,
        vec![
            "font 'Poppins': not installed; the editor will download it from Google Fonts when \
             auto-download is on (otherwise it falls back to a default)"
        ]
    );
    let issues = scan_font_issues(&page_of(&["Proprietary Sans"]), &known);
    assert!(
        issues[0].contains("licence does not allow automatic download"),
        "{issues:?}"
    );
}

/// Invariant: bundled families, imported / system ones the host reports, CSS
/// generics and a stack with one resolvable member are never flagged.
#[test]
fn bundled_and_generic_families_are_not_flagged() {
    let known = live(Some(sample_catalog()));
    let page = page_of(&[
        "Inter",
        "dm sans",
        "Roboto",
        "Helvetica Neue",
        "sans-serif",
        "system-ui",
        "Poppins, Inter",
        "'Unknown Display', serif",
    ]);
    assert!(scan_font_issues(&page, &known).is_empty());
    // An imported font the host registered counts too.
    let imported = FontKnowledge::new(["My Imported Face"], true, None);
    assert!(scan_font_issues(&page_of(&["my imported face"]), &imported).is_empty());
}

/// Invariant: a style-variable reference or `inherit` is not a family name, so
/// it is never reported, in a node or in a component override.
#[test]
fn style_variable_font_is_not_flagged() {
    let known = live(Some(sample_catalog()));
    let page = page_of(&["$--font-heading", "inherit", "  "]);
    assert!(scan_font_issues(&page, &known).is_empty());
    let with_override: Vec<PenNode> = serde_json::from_value(json!([
        { "type": "ref", "id": "r1", "ref": "button", "width": 10, "height": 10,
          "descendants": { "label": { "fontFamily": "$--font-body" } } }
    ]))
    .expect("nodes");
    assert!(scan_font_issues(&with_override, &known).is_empty());
}

/// Invariant: a family used by many text nodes (and in a component override,
/// and spelled in another case) is reported once.
#[test]
fn one_line_per_distinct_family() {
    let known = live(Some(sample_catalog()));
    let mut nodes = page_of(&["Zzyzx Sans", "zzyzx sans", "Zzyzx Sans", "Qwerty Serif"]);
    let override_node: Vec<PenNode> = serde_json::from_value(json!([
        { "type": "ref", "id": "r1", "ref": "button", "width": 10, "height": 10,
          "descendants": { "label": { "fontFamily": "Zzyzx Sans" } } }
    ]))
    .expect("nodes");
    nodes.extend(override_node);
    let issues = scan_font_issues(&nodes, &known);
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert!(issues
        .iter()
        .any(|line| line.starts_with("font 'Qwerty Serif'")));
    assert_eq!(
        issues
            .iter()
            .filter(|line| line.to_lowercase().contains("zzyzx sans"))
            .count(),
        1
    );
}

/// Invariant: without system-font knowledge only names found in no list are
/// reported, worded as "unknown, system fonts not checked"; a catalog family
/// (it may be installed) and a stand-in system font are silent; with no
/// catalog either nothing can be judged.
#[test]
fn headless_without_system_fonts_flags_only_unknown_names() {
    let known = headless(Some(sample_catalog()));
    let issues = scan_font_issues(&page_of(&["Poppins", "Arial", "Inter", "Poppin"]), &known);
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].starts_with("font 'Poppin': unknown family (not bundled, not in the Google Fonts catalog; system fonts not checked); closest: Poppins"));
    assert!(!issues[0].contains("not installed"));
    let none = headless(None);
    assert!(scan_font_issues(&page_of(&["Poppin", "Zzyzx"]), &none).is_empty());
}

/// Invariant: a missing file, garbage and an empty array are all "no
/// catalog" (never an error); a valid file is parsed once and re-read only
/// after it changes.
#[test]
fn garbled_or_missing_catalog_cache_is_ignored() {
    let cache = CatalogFileCache::new();
    let missing = std::env::temp_dir().join("op-font-issues-definitely-missing.json");
    assert!(cache.get(&missing).is_none());
    assert_eq!(cache.parse_count(), 0, "a missing file is not parsed");

    let garbled = scratch_file("garbled", b"{ not json");
    assert!(cache.get(&garbled).is_none());
    assert!(cache.get(&garbled).is_none());
    assert_eq!(
        cache.parse_count(),
        1,
        "garbage is parsed once, not per call"
    );
    let empty = scratch_file("empty", br#"{"fetchedAtUnix":1,"catalog":[]}"#);
    assert!(cache.get(&empty).is_none());

    // The feedback path degrades to "no catalog" rather than failing.
    let mut state = EditorState::new();
    state.editor_ui.system_fonts_loaded = true;
    let issues = super::with_font_catalog_file(Some(garbled), || {
        scan_font_issues(&page_of(&["Zzyzx"]), &FontKnowledge::from_state(&state))
    });
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(issues[0].starts_with("font 'Zzyzx': not installed; closest: "));

    // A real catalog file is parsed once across calls.
    let file = json!({ "fetchedAtUnix": 1, "etag": null, "catalog": [
        { "id": "sora", "family": "Sora", "license": "OFL-1.1", "type": "google" }
    ] });
    let good = scratch_file("good", file.to_string().as_bytes());
    let first = cache.get(&good).expect("catalog");
    let second = cache.get(&good).expect("catalog");
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(
        first.lookup("sora").map(|entry| entry.id.as_str()),
        Some("sora")
    );
    assert_eq!(cache.parse_count(), 3);

    // The same file reaches the scan through the process-level lookup.
    let issues = super::with_font_catalog_file(Some(good), || {
        scan_font_issues(&page_of(&["Sora"]), &FontKnowledge::from_state(&state))
    });
    assert!(
        issues[0].contains("will download it from Google Fonts"),
        "{issues:?}"
    );
}

/// Invariant: the in-app diagnostics and the shared function key the issues
/// under `fontIssues`, with the same lines, built from the editor state's own
/// font snapshot; a page of installed fonts adds no key at all.
#[test]
fn font_issues_ride_the_shared_diagnostics_key() {
    let mut state = EditorState::new();
    let nodes = page_of(&["Zzyzx Sans"]);
    state.doc.children = nodes;
    state.editor_ui.system_fonts_loaded = true;
    state.editor_ui.system_font_families = Arc::new(vec!["Menlo".into()]);
    let diagnostics = super::with_font_catalog_file(None, || {
        post_write_diagnostics(&state, PostWriteExtras::default(), Some(&MCP_FEEDBACK_CAPS))
    });
    let lines = diagnostics["fontIssues"].as_array().expect("fontIssues");
    assert_eq!(lines.len(), 1);
    assert!(lines[0]
        .as_str()
        .expect("line")
        .starts_with("font 'Zzyzx Sans': not installed"));

    let mut clean = EditorState::new();
    clean.doc.children = serde_json::from_str(&format!(
        r##"[{{"type":"frame","id":"root","width":10,"height":10,"children":[{CLEAN_FAMILY_NODE}]}}]"##
    ))
    .expect("nodes");
    let diagnostics = super::with_font_catalog_file(None, || {
        post_write_diagnostics(&clean, PostWriteExtras::default(), Some(&MCP_FEEDBACK_CAPS))
    });
    assert!(!diagnostics.contains_key("fontIssues"));
}
