//! Write-time auto-rename of misspelled glyph names (`icon_auto_rename`).

use serde_json::json;

use super::icon_names::rename_target;
use super::*;

fn state_with_icons(icons: &[(&str, &str, Option<&str>)]) -> EditorState {
    let children: Vec<serde_json::Value> = icons
        .iter()
        .map(|(id, name, family)| {
            let mut node = json!({
                "type": "icon_font", "id": id, "iconFontName": name,
                "width": 20, "height": 20
            });
            if let Some(family) = family {
                node["iconFontFamily"] = json!(family);
            }
            node
        })
        .collect();
    EditorState::from_document(
        serde_json::from_value(json!({
            "version": "1.0",
            "children": [{ "type": "frame", "id": "root", "width": 200, "height": 200,
                           "children": children }]
        }))
        .expect("document"),
    )
}

fn icon_of(state: &EditorState, id: &str) -> (String, Option<String>) {
    fn find(nodes: &[PenNode], id: &str) -> Option<(String, Option<String>)> {
        for node in nodes {
            if let PenNode::IconFont(icon) = node {
                if icon.base.id == id {
                    return Some((icon.icon_font_name.clone(), icon.icon_font_family.clone()));
                }
            }
            if let Some(found) = node.children().and_then(|kids| find(kids, id)) {
                return Some(found);
            }
        }
        None
    }
    find(state.active_children(), id).expect("icon node")
}

/// Rename every icon of `state` as if the whole page were just written.
fn rename_all(state: &mut EditorState) -> Vec<IconRename> {
    rename_unknown_icons(state, &IconNameSnapshot::default())
}

#[test]
fn a_name_in_another_case_or_separator_style_is_renamed_to_the_one_that_paints() {
    for (written, expected) in [
        ("Search", "search"),
        ("Shopping_Cart", "shopping-cart"),
        ("check circle", "check-circle"),
    ] {
        assert_eq!(
            rename_target(None, written).as_deref(),
            Some(expected),
            "{written}"
        );
    }
    let mut state = state_with_icons(&[("a", "Shopping_Cart", None)]);
    let renames = rename_all(&mut state);
    assert_eq!(icon_of(&state, "a").0, "shopping-cart");
    assert_eq!(
        renames,
        vec![IconRename {
            node_id: "a".into(),
            from: "Shopping_Cart".into(),
            to: "shopping-cart".into()
        }]
    );
    assert_eq!(
        renames[0].note(),
        "renamed: Shopping_Cart -> shopping-cart (icon a)"
    );
}

#[test]
fn a_single_typo_away_from_one_glyph_is_renamed() {
    for (written, expected) in [
        ("serch", "search"),
        ("heartt", "heart"),
        ("calender", "calendar"),
    ] {
        assert_eq!(
            rename_target(None, written).as_deref(),
            Some(expected),
            "{written}"
        );
    }
    let mut state = state_with_icons(&[("a", "serch", None)]);
    rename_all(&mut state);
    assert_eq!(icon_of(&state, "a").0, "search");
}

#[test]
fn an_ambiguous_name_is_kept() {
    // Two real glyphs are one edit from "bel" + "l": pick names that reach
    // more than one glyph. `chevron-lef` is one edit from `chevron-left`
    // only, so build the ambiguity from the catalog itself.
    let names: Vec<&str> = op_editor_ui::widgets::icon_catalog::core_icon_names("lucide").collect();
    let ambiguous = names
        .iter()
        .flat_map(|first| {
            names
                .iter()
                .filter(move |second| {
                    first < *second && first.len() >= 5 && second.len() == first.len()
                })
                .filter_map(move |second| {
                    let differing: Vec<usize> = first
                        .chars()
                        .zip(second.chars())
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .map(|(at, _)| at)
                        .collect();
                    (differing.len() == 1).then(|| {
                        // A name that differs from both at the same position.
                        let at = differing[0];
                        let mut mid: Vec<char> = first.chars().collect();
                        mid[at] = '#';
                        mid.into_iter().collect::<String>()
                    })
                })
        })
        .next()
        .expect("two lucide glyphs one substitution apart");
    assert_eq!(rename_target(None, &ambiguous), None, "{ambiguous}");
    let mut state = state_with_icons(&[("a", &ambiguous, None)]);
    assert!(rename_all(&mut state).is_empty());
    assert_eq!(icon_of(&state, "a").0, ambiguous);
}

#[test]
fn an_unknown_or_too_short_name_is_kept_and_still_reported() {
    for written in [
        "definitely-not-a-real-icon",
        "zzzzqqqq",
        "cat",
        "ab",
        "",
        "$icon-var",
    ] {
        assert_eq!(rename_target(None, written), None, "{written:?}");
    }
    let mut state = state_with_icons(&[("a", "definitely-not-a-real-icon", None)]);
    assert!(rename_all(&mut state).is_empty());
    assert_eq!(icon_of(&state, "a").0, "definitely-not-a-real-icon");
    let feedback = post_write_diagnostics(&state, PostWriteExtras::default(), None);
    assert!(feedback["iconIssues"][0]
        .as_str()
        .expect("line")
        .contains("'definitely-not-a-real-icon' is not a lucide glyph"));
}

#[test]
fn names_the_painter_already_draws_are_left_exactly_as_written() {
    // Legacy aliases, the chrome table's own aliases and the feather fallback
    // all paint today: rewriting them would change a clean write.
    let written = [
        "check-circle",
        "alert-triangle",
        "home",
        "cart",
        "favorite",
        "search",
        "x",
    ];
    let mut state = state_with_icons(
        &written
            .iter()
            .enumerate()
            .map(|(index, name)| (["a", "b", "c", "d", "e", "f", "g"][index], *name, None))
            .collect::<Vec<_>>(),
    );
    assert!(rename_all(&mut state).is_empty());
    for (index, name) in written.iter().enumerate() {
        assert_eq!(
            icon_of(&state, ["a", "b", "c", "d", "e", "f", "g"][index]).0,
            *name
        );
    }
    let feedback = post_write_diagnostics(&state, PostWriteExtras::default(), None);
    assert!(feedback.get("iconIssues").is_none(), "{feedback:?}");
}

#[test]
fn only_the_name_is_rewritten_never_the_family_or_other_nodes() {
    // A family value is never rewritten, and a family the editor has no glyph
    // list for (a brand set, an unknown family) is never judged.
    let mut state = state_with_icons(&[
        ("a", "serch", Some("lucide")),
        ("b", "serch", Some("Material Symbols Rounded")),
        ("c", "githubb", Some("simple-icons")),
    ]);
    let renames = rename_all(&mut state);
    assert_eq!(renames.len(), 1);
    assert_eq!(
        icon_of(&state, "a"),
        ("search".into(), Some("lucide".into()))
    );
    assert_eq!(
        icon_of(&state, "b"),
        ("serch".into(), Some("Material Symbols Rounded".into()))
    );
    assert_eq!(icon_of(&state, "c").0, "githubb");
    // A node that is not an icon is never touched, whatever its name says.
    let mut state: EditorState = EditorState::from_document(
        serde_json::from_value(json!({"version":"1.0","children":[
            {"type":"text","id":"t","name":"serch","content":"serch","width":80,"height":20}
        ]}))
        .expect("document"),
    );
    assert!(rename_all(&mut state).is_empty());
}

#[test]
fn an_icon_that_was_already_on_the_page_is_not_rewritten_by_a_later_write() {
    let mut state = state_with_icons(&[("old", "serch", None)]);
    let before = IconNameSnapshot::capture(&state);
    // The write adds a new misspelled icon and leaves "old" alone.
    let nodes = state.active_children_mut();
    if let Some(root) = nodes.first_mut().and_then(|node| node.children_mut()) {
        root.push(
            serde_json::from_value(
                json!({"type":"icon_font","id":"new","iconFontName":"heartt",
                "width":20,"height":20}),
            )
            .expect("node"),
        );
    }
    let renames = rename_unknown_icons(&mut state, &before);
    assert_eq!(renames.len(), 1);
    assert_eq!(icon_of(&state, "new").0, "heart");
    assert_eq!(icon_of(&state, "old").0, "serch");
    // An icon whose name the write itself changed is eligible.
    let before = IconNameSnapshot::capture(&state);
    if let Some(PenNode::Frame(root)) = state.active_children_mut().first_mut() {
        if let Some(PenNode::IconFont(icon)) = root.children.as_mut().and_then(|k| k.first_mut()) {
            icon.icon_font_name = "calender".into();
        }
    }
    let renames = rename_unknown_icons(&mut state, &before);
    assert_eq!(renames.len(), 1);
    assert_eq!(icon_of(&state, "old").0, "calendar");
}

#[test]
fn renames_are_reported_first_in_icon_issues_without_the_fallback_hint() {
    let state = state_with_icons(&[("a", "search", None)]);
    let notes = vec!["renamed: serch -> search (icon a)".to_string()];
    let feedback = post_write_diagnostics(
        &state,
        PostWriteExtras {
            icon_notes: notes.clone(),
            ..PostWriteExtras::default()
        },
        None,
    );
    assert_eq!(feedback["iconIssues"], json!(notes));
    assert!(feedback.get("layoutHint").is_none(), "{feedback:?}");

    // Next to a name that stays unresolved the hint is back, and the rename
    // note still leads.
    let state = state_with_icons(&[("a", "search", None), ("b", "zzzzqqqq", None)]);
    let feedback = post_write_diagnostics(
        &state,
        PostWriteExtras {
            icon_notes: notes,
            ..PostWriteExtras::default()
        },
        None,
    );
    let lines = feedback["iconIssues"].as_array().expect("lines");
    assert_eq!(lines.len(), 2);
    assert!(lines[0].as_str().expect("line").starts_with("renamed: "));
    assert!(feedback["layoutHint"]
        .as_str()
        .expect("hint")
        .contains("iconIssues"));
}

#[test]
fn recording_logs_renames_only_while_alive_and_is_bounded() {
    let mut state = state_with_icons(&[("a", "serch", None)]);
    assert_eq!(rename_all(&mut state).len(), 1);
    let recording = RenameRecording::start();
    let mut state = state_with_icons(&[("a", "serch", None), ("b", "heartt", None)]);
    rename_all(&mut state);
    let logged = recording.take();
    assert_eq!(logged.len(), 2);
    assert!(recording.take().is_empty(), "take restarts the log");
    drop(recording);
    let mut state = state_with_icons(&[("a", "serch", None)]);
    rename_all(&mut state);
    let recording = RenameRecording::start();
    assert!(
        recording.take().is_empty(),
        "nothing logged outside a recording"
    );
}

/// The `iconFontName` of the one icon on the page.
fn only_icon_name(state: &EditorState) -> String {
    fn find(nodes: &[PenNode]) -> Option<String> {
        nodes.iter().find_map(|node| match node {
            PenNode::IconFont(icon) => Some(icon.icon_font_name.clone()),
            other => other.children().and_then(|kids| find(kids)),
        })
    }
    find(state.active_children()).expect("an icon on the page")
}

fn batch_args(icon_name: &str) -> String {
    let script = format!(
        r#"I(null, {{"type":"frame","id":"card","width":200,"height":100,"layout":"none","children":[{{"type":"icon_font","id":"star","iconFontName":"{icon_name}","width":20,"height":20}}]}});"#
    );
    json!({ "script": script }).to_string()
}

#[test]
fn the_in_app_batch_design_renames_the_document_and_reports_it() {
    let mut state = EditorState::new();
    let (result, mutated) = execute_design_tool(&mut state, "batch_design", &batch_args("serch"));
    assert!(mutated && !result.is_error, "{}", result.content);
    let envelope: serde_json::Value = serde_json::from_str(&result.content).expect("json");
    // The batch assigns its own node ids, so the line is checked by its shape
    // and the document by the only icon it holds.
    let lines = envelope["iconIssues"].as_array().expect("iconIssues");
    assert_eq!(lines.len(), 1, "{envelope}");
    assert!(lines[0]
        .as_str()
        .expect("line")
        .starts_with("renamed: serch -> search (icon "));
    assert_eq!(only_icon_name(&state), "search");
    assert!(
        envelope.get("layoutHint").is_none(),
        "a rename alone does not ask the model to fix icons: {envelope}"
    );
}

#[test]
fn the_in_app_batch_design_keeps_and_reports_an_unknown_name_and_leaves_clean_writes_alone() {
    let mut state = EditorState::new();
    let (result, _) = execute_design_tool(&mut state, "batch_design", &batch_args("zzzzqqqq"));
    let envelope: serde_json::Value = serde_json::from_str(&result.content).expect("json");
    assert!(envelope["iconIssues"][0]
        .as_str()
        .expect("line")
        .contains("'zzzzqqqq' is not a lucide glyph"));
    assert_eq!(only_icon_name(&state), "zzzzqqqq");

    for alias in ["check-circle", "cart", "search"] {
        let mut state = EditorState::new();
        let (result, _) = execute_design_tool(&mut state, "batch_design", &batch_args(alias));
        assert!(
            !result.content.contains("iconIssues"),
            "{alias}: {}",
            result.content
        );
        assert_eq!(only_icon_name(&state), alias);
    }
}
