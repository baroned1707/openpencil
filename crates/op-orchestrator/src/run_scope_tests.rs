//! The scope computation: which sections a run touched, and when it refuses to
//! scope at all.

use super::fixtures::*;
use super::*;
use op_editor_core::{EditorCommand, EditorState, PenNodeExt, RunBaseline};
use serde_json::json;

fn touched(scope: &RunScope) -> Vec<&str> {
    scope
        .touched_sections()
        .expect("a scoped result")
        .iter()
        .map(String::as_str)
        .collect()
}

fn since(baseline: &RunBaseline, run: &EditorState) -> RunScope {
    RunScope::since(Some(baseline), run)
}

/// Invariant: a document no one edited has an empty scope, not a whole-page
/// one. If serialization noise made every section look changed, the scoped
/// finalizer would silently do nothing but fall back to the whole page.
#[test]
fn an_unedited_document_has_an_empty_scope() {
    let base = page();
    let baseline = RunBaseline::capture(&base);
    let scope = since(&baseline, &base);
    assert!(touched(&scope).is_empty(), "{scope:?}");
    // The same document loaded again (a fresh parse, as a seed file would be).
    let reloaded = page();
    assert_eq!(since(&baseline, &reloaded), scope);
}

/// Invariant: an edit that keeps every id (the `U()` an agent uses to modify)
/// touches the section that contains it, and only that one.
#[test]
fn a_same_id_edit_touches_its_section_only() {
    let base = page();
    let baseline = RunBaseline::capture(&base);
    let mut run = base.clone();
    patch(&mut run, "hero-title", json!({ "fontSize": 72 }));
    assert_eq!(touched(&since(&baseline, &run)), ["hero"]);

    patch(&mut run, "f2-b", json!({ "content": "Changed." }));
    assert_eq!(touched(&since(&baseline, &run)), ["hero", "features"]);
}

/// Invariant: adding a child to a section touches that section; the new
/// section an agent appends to the page is itself touched.
#[test]
fn an_added_node_and_an_added_section_are_touched() {
    let base = page();
    let baseline = RunBaseline::capture(&base);
    let mut run = base.clone();
    insert(
        &mut run,
        "hero",
        text("hero-note", "Booking.", 15, "#111827"),
    );
    assert_eq!(touched(&since(&baseline, &run)), ["hero"]);

    let footer = insert(
        &mut run,
        "page",
        json!({ "type": "frame", "id": "footer", "name": "Footer", "layout": "vertical",
                "children": [text("footer-t", "Bye", 14, "#111827")] }),
    );
    assert_eq!(
        touched(&since(&baseline, &run)),
        ["hero", footer.as_str()],
        "the appended section is a touched section of its own"
    );
}

/// Invariant: a removed or moved node touches the sections it left and
/// entered; a removed whole section leaves nothing to repair.
#[test]
fn removals_and_moves_touch_the_sections_they_change() {
    let base = page();
    let baseline = RunBaseline::capture(&base);

    let mut removed_node = base.clone();
    assert!(removed_node.apply(EditorCommand::DeleteNode {
        node_id: NodeId::new("hero-sub"),
        page_id: None,
    }));
    assert_eq!(touched(&since(&baseline, &removed_node)), ["hero"]);

    let mut removed_section = base.clone();
    assert!(removed_section.apply(EditorCommand::DeleteNode {
        node_id: NodeId::new("features"),
        page_id: None,
    }));
    assert!(touched(&since(&baseline, &removed_section)).is_empty());

    let mut moved = base.clone();
    assert!(moved.apply(EditorCommand::MoveNode {
        node_id: NodeId::new("nav-l2"),
        target_parent: NodeId::new("hero"),
        page_id: None,
        index: None,
    }));
    assert_eq!(touched(&since(&baseline, &moved)), ["nav", "hero"]);
}

/// Invariant: a run that built the design is never scoped. A new top-level
/// frame, a baseline with no content (blank canvas, the starter frame, an
/// empty scaffold), no baseline at all, and a page switch are all the whole page.
#[test]
fn a_run_that_built_the_design_is_the_whole_page() {
    let base = page();
    let baseline = RunBaseline::capture(&base);

    let mut second_screen = base.clone();
    insert_root(
        &mut second_screen,
        json!({ "type": "frame", "id": "screen-2",
        "children": [text("s2", "Two", 14, "#111827")] }),
    );
    assert_eq!(since(&baseline, &second_screen), RunScope::Whole);

    // The starter frame is reused by the run: its id never changes.
    let starter = EditorState::starter();
    let blank = RunBaseline::capture(&starter);
    let mut filled = starter.clone();
    let root = filled.active_children()[0].id_str().to_string();
    insert(&mut filled, &root, text("t", "Hello", 14, "#111827"));
    assert_eq!(since(&blank, &filled), RunScope::Whole);

    assert_eq!(RunScope::since(None, &base), RunScope::Whole);

    let mut other_page = base.clone();
    other_page.ui.active_page_index = 1;
    assert_eq!(since(&baseline, &other_page), RunScope::Whole);
}

fn insert_root(state: &mut EditorState, node: serde_json::Value) {
    let node: jian_ops_schema::node::PenNode = serde_json::from_value(node).expect("root");
    state.active_children_mut().push(node);
}

/// Invariant: when the run changed the page wrapper itself, reordered its
/// sections, or rewrote every section, there is nothing to hold back.
#[test]
fn page_level_and_rewrite_everything_runs_are_the_whole_page() {
    let base = page();
    let baseline = RunBaseline::capture(&base);

    let mut recoloured = base.clone();
    patch(
        &mut recoloured,
        "page",
        json!({ "fill": [{ "type": "solid", "color": "#0B0B0B" }] }),
    );
    assert_eq!(since(&baseline, &recoloured), RunScope::Whole);

    let mut reordered = base.clone();
    assert!(reordered.apply(EditorCommand::MoveNode {
        node_id: NodeId::new("features"),
        target_parent: NodeId::new("page"),
        page_id: None,
        index: Some(0),
    }));
    assert_eq!(since(&baseline, &reordered), RunScope::Whole);

    let mut rewritten = base.clone();
    for id in ["nav-logo", "hero-title", "features-title"] {
        patch(&mut rewritten, id, json!({ "content": "New" }));
    }
    assert_eq!(since(&baseline, &rewritten), RunScope::Whole);
}

/// Invariant: in a multi-screen document (no page wrapper) the unit is the
/// top-level screen.
#[test]
fn a_flat_document_scopes_by_screen() {
    let screen = |id: &str| {
        json!({ "type": "frame", "id": id, "name": id, "width": 390, "height": 800,
                "layout": "vertical",
                "children": [text(&format!("{id}-t"), "Hi", 16, "#111827")] })
    };
    let base = state_from(json!({ "version": "1.0",
        "children": [screen("a"), screen("b"), screen("c")] }));
    let baseline = RunBaseline::capture(&base);
    let mut run = base.clone();
    patch(&mut run, "b-t", json!({ "content": "Changed" }));
    assert_eq!(touched(&since(&baseline, &run)), ["b"]);
}

/// Invariant: `root_ids` map to the section that contains each id; naming the
/// wrapper or every section is the whole page; an unknown id is an error.
#[test]
fn root_ids_map_to_sections() {
    let base = page();
    let ids = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    let scope = |names: &[&str]| RunScope::from_root_ids(&base, &ids(names));

    assert_eq!(touched(&scope(&["hero"]).unwrap()), ["hero"]);
    assert_eq!(touched(&scope(&["f3-b"]).unwrap()), ["features"]);
    assert_eq!(
        touched(&scope(&["hero-title", "nav-logo", "hero"]).unwrap()),
        ["nav", "hero"],
        "deduplicated, in document order"
    );
    assert_eq!(scope(&["page"]).unwrap(), RunScope::Whole);
    assert_eq!(
        scope(&["nav", "hero", "features"]).unwrap(),
        RunScope::Whole
    );
    assert_eq!(
        scope(&["hero", "nope"]),
        Err(RunScopeError::UnknownIds(ids(&["nope"])))
    );
    assert_eq!(scope(&[]), Err(RunScopeError::NoIds));
}

/// Invariant: an editor write that turns "no children" into an empty list
/// (an empty image-slot rectangle does this on any insert) does not make the
/// section that holds it look edited.
#[test]
fn an_empty_children_list_is_not_a_content_change() {
    let slot = json!({ "type": "rectangle", "id": "photo", "width": 240, "height": 160,
                       "fill": [{ "type": "image", "url": "" }] });
    let mut doc = page_json("#111827");
    doc["children"][0]["children"][0]["children"]
        .as_array_mut()
        .expect("nav children")
        .push(slot);
    let base = state_from(doc);
    let baseline = RunBaseline::capture(&base);
    let mut run = base.clone();
    insert(
        &mut run,
        "hero",
        text("hero-note", "Booking.", 15, "#111827"),
    );
    assert_eq!(touched(&since(&baseline, &run)), ["hero"]);
}
