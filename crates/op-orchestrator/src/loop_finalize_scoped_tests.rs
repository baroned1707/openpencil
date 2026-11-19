//! The scoped (modify-run) loop finalizer: what it leaves alone, what it still
//! repairs, and that a run that built the design finalizes exactly as before.

use super::{apply_loop_finalize_counted, apply_loop_finalize_since, record_loop_finalize_scoped};
use crate::run_scope::fixtures::*;
use crate::run_scope::RunScope;
use op_editor_core::{EditorCommand, EditorState, RunBaseline};
use serde_json::{json, Value};

/// A run that starts from `base`, applies `edit`, and ends with the finalize.
fn run(base: &EditorState, edit: impl FnOnce(&mut EditorState)) -> (EditorState, EditorState) {
    let baseline = RunBaseline::capture(base);
    let mut edited = base.clone();
    edit(&mut edited);
    let mut finalized = edited.clone();
    let _ = apply_loop_finalize_since(&mut finalized, Some(&baseline));
    (edited, finalized)
}

fn hero_edit(state: &mut EditorState) {
    patch(
        state,
        "hero-title",
        json!({ "fontSize": 72, "fontWeight": "800" }),
    );
}

/// Invariant: a run that touched only the hero leaves the navigation, the
/// features, the page frame's gap and every role outside the hero exactly as
/// the run left them. The precondition is checked on the same document: the
/// whole-page finalize DOES change them, so this test cannot pass by accident.
#[test]
fn a_hero_only_run_leaves_the_rest_of_the_page_alone() {
    let base = page();
    let (edited, finalized) = run(&base, hero_edit);

    let mut whole = edited.clone();
    let _ = apply_loop_finalize_counted(&mut whole);
    let outside = [
        "page",
        "nav",
        "nav-logo",
        "nav-links",
        "features",
        "features-row",
        "f1",
        "f2",
        "f3",
        "f1-t",
        "f3-b",
    ];
    let leaked: Vec<&str> = outside
        .iter()
        .copied()
        .filter(|id| own(&whole, id) != own(&edited, id))
        .collect();
    assert!(
        leaked.contains(&"page") && leaked.contains(&"nav") && leaked.contains(&"features"),
        "precondition: the whole-page finalize restyles the untouched sections: {leaked:?}"
    );

    for id in outside {
        assert_eq!(
            own(&finalized, id),
            own(&edited, id),
            "{id} was not touched"
        );
    }
    assert!(own(&finalized, "page").get("gap").is_none(), "no page gap");
}

/// Invariant: the scoped result is reproduced command for command by the
/// recorder the MCP host replays, and the ledger says the scope was narrowed.
#[test]
fn the_recorder_reproduces_the_scoped_result() {
    let base = page();
    let baseline = RunBaseline::capture(&base);
    let mut edited = base.clone();
    hero_edit(&mut edited);
    insert(&mut edited, "hero", note_without_fill());
    let scope = RunScope::since(Some(&baseline), &edited);
    assert_eq!(scope.touched_sections(), Some(&["hero".to_string()][..]));

    let mut direct = edited.clone();
    let summary = apply_loop_finalize_since(&mut direct, Some(&baseline));
    let recorded = record_loop_finalize_scoped(&edited, &scope).expect("recorded");
    assert_eq!(canonical(&recorded.state), canonical(&direct));
    let mut replay = edited.clone();
    assert!(replay.apply(EditorCommand::Batch {
        commands: recorded.commands,
    }));
    assert_eq!(canonical(&replay), canonical(&direct));
    assert_eq!(recorded.summary.notes(), summary.notes());
    assert!(
        summary
            .notes()
            .iter()
            .any(|n| n.contains("finalize scoped to the 1 section")),
        "{:?}",
        summary.notes()
    );
}

fn note_without_fill() -> Value {
    json!({ "type": "text", "id": "hero-note", "content": "Booking now.",
            "fontFamily": "Inter", "fontSize": 15,
            "width": "fit_content", "height": "fit_content" })
}

/// Invariant: a repair INSIDE the touched section still happens. A fill-less
/// text (a semantic, token-tier repair) gets a fill, and low-contrast text
/// (contract tier) is recoloured.
#[test]
fn a_needed_repair_inside_the_touched_section_still_happens() {
    let base = page();
    let mut note_id = String::new();
    let (_, finalized) = run(&base, |state| {
        note_id = insert(state, "hero", note_without_fill());
        patch(
            state,
            "hero-sub",
            json!({ "fill": [{ "type": "solid", "color": "#F1F2F4" }] }),
        );
    });
    let fill = |id: &str| own(&finalized, id)["fill"].clone();
    assert!(
        fill(&note_id).as_array().is_some_and(|f| !f.is_empty()),
        "the fill-less text in the touched section gets a fill: {}",
        fill(&note_id)
    );
    assert_ne!(
        fill("hero-sub"),
        json!([{ "type": "solid", "color": "#F1F2F4" }]),
        "near-invisible text in the touched section is recoloured"
    );
}

/// Invariant: an overflow the run introduced inside the touched section is
/// repaired by the geometry sweep, and the untouched sections stay as they were.
#[test]
fn an_overflow_inside_the_touched_section_is_repaired() {
    let base = page();
    let mut wide_id = String::new();
    let (edited, finalized) = run(&base, |state| {
        wide_id = insert(
            state,
            "hero",
            json!({ "type": "frame", "id": "wide", "name": "Wide strip", "layout": "horizontal",
                    "width": 3200, "height": 48,
                    "fill": [{ "type": "solid", "color": "#E5E7EB" }],
                    "children": [text("wide-t", "Overflowing strip", 16, "#111827")] }),
        );
    });
    let width = |state: &EditorState| own(state, &wide_id)["width"].clone();
    assert_eq!(width(&edited), json!(3200.0));
    assert_ne!(
        width(&finalized),
        width(&edited),
        "the strip is pulled back inside"
    );
    assert_eq!(own(&finalized, "nav"), own(&edited, "nav"));
}

/// Invariant: an empty image-fill slot the run wrote becomes an image node (the
/// enrichment step only fills those), while the same shape in a section the
/// run never touched is left as it was.
#[test]
fn an_empty_image_slot_in_the_touched_section_is_materialized() {
    let slot = |id: &str| {
        json!({ "type": "rectangle", "id": id, "name": "Photo", "width": 240, "height": 160,
                "fill": [{ "type": "image", "url": "" }] })
    };
    let mut base_doc = page_json("#111827");
    base_doc["children"][0]["children"][0]["children"]
        .as_array_mut()
        .expect("nav children")
        .push(slot("nav-photo"));
    let base = state_from(base_doc);
    let mut photo = String::new();
    assert_eq!(
        own(&base, "nav-photo")["type"],
        "rectangle",
        "fixture loads as a slot"
    );
    let (edited, finalized) = run(&base, |state| {
        photo = insert(state, "hero", slot("hero-photo"));
    });
    assert_eq!(
        own(&edited, "nav-photo")["type"],
        "rectangle",
        "the run did not touch it"
    );
    assert_eq!(
        own(&finalized, &photo)["type"],
        "image",
        "{}",
        own(&finalized, &photo)
    );
    assert_eq!(own(&finalized, "nav-photo")["type"], "rectangle");
}

/// Contract edge, decided and pinned: the contract sweep covers the WHOLE
/// page, so a text-on-background defect outside the touched sections is
/// repaired whether the run caused it or it was already there. What a scope
/// protects is the INTENT tier (roles, gaps, fills): the repaired node keeps
/// everything else it had, and its section gets no role.
#[test]
fn the_contract_sweep_repairs_a_defect_outside_the_scope() {
    // Already there: the navigation logo was near-white on white before the run.
    let base = state_from(page_json("#FAFAFA"));
    let (edited, finalized) = run(&base, hero_edit);
    assert_ne!(
        own(&finalized, "nav-logo")["fill"],
        own(&edited, "nav-logo")["fill"],
        "a pre-existing contrast defect outside the scope is repaired"
    );
    assert_eq!(
        own(&finalized, "nav"),
        own(&edited, "nav"),
        "no role for nav"
    );
    assert_eq!(own(&finalized, "page"), own(&edited, "page"));
}

/// The run broke it: the logo is bound to a colour variable and the run
/// repaints that variable to near-white. The logo node itself is unchanged, so
/// its section is not touched, and the sweep still repairs it.
#[test]
fn the_contract_sweep_repairs_what_the_run_broke_elsewhere() {
    let mut doc = page_json("$--ink");
    doc["variables"]["--ink"]["value"] = json!("#111827");
    let base = state_from(doc);
    let (_, finalized) = run(&base, |state| {
        hero_edit(state);
        assert!(state.apply(EditorCommand::SetVariableColor {
            name: "--ink".into(),
            hex: "#FAFAFA".into(),
        }));
    });
    let fill = own(&finalized, "nav-logo")["fill"].clone();
    assert_ne!(
        fill,
        json!([{ "type": "solid", "color": "$--ink" }]),
        "text that the run made invisible is repaired: {fill}"
    );
}

/// Invariant: an empty scope is not a skip. Nothing in the forest changed, no
/// section gets a semantic repair, and the contract sweep still runs.
#[test]
fn an_empty_scope_still_runs_the_contract_sweep() {
    let base = state_from(page_json("#FAFAFA"));
    let baseline = RunBaseline::capture(&base);
    let mut finalized = base.clone();
    let summary = apply_loop_finalize_since(&mut finalized, Some(&baseline));
    assert_ne!(
        own(&finalized, "nav-logo")["fill"],
        own(&base, "nav-logo")["fill"]
    );
    for id in ["page", "nav", "hero", "features", "f1"] {
        assert_eq!(own(&finalized, id), own(&base, id), "{id}");
    }
    assert!(
        summary
            .notes()
            .iter()
            .any(|n| n.contains("no section was changed")),
        "{:?}",
        summary.notes()
    );
}

/// Invariant: a run that BUILT the design finalizes exactly as before (same
/// document, same ledger): a new top-level frame, the starter frame reused in
/// place, and a call with no baseline.
#[test]
fn a_run_that_builds_the_design_finalizes_exactly_as_before() {
    let finish = |baseline: Option<&RunBaseline>, state: &EditorState| {
        let mut since = state.clone();
        let summary = apply_loop_finalize_since(&mut since, baseline);
        let mut counted = state.clone();
        let reference = apply_loop_finalize_counted(&mut counted);
        assert_eq!(canonical(&since), canonical(&counted));
        assert_eq!(summary.notes(), reference.notes());
        assert_eq!(summary.total_repairs(), reference.total_repairs());
        canonical(&since)
    };

    // A new page on an empty document.
    let empty = EditorState::new();
    let blank = RunBaseline::capture(&empty);
    let mut built = page();
    built.ui.active_page_index = 0;
    finish(Some(&blank), &built);

    // A new screen next to an existing page.
    let existing = page();
    let baseline = RunBaseline::capture(&existing);
    let mut with_screen = existing.clone();
    let screen: jian_ops_schema::node::PenNode = serde_json::from_value(json!({
        "type": "frame", "id": "screen-2", "name": "Screen 2", "width": 390, "height": 800,
        "layout": "vertical", "children": [text("s2", "Two", 16, "#111827")]
    }))
    .expect("screen");
    with_screen.active_children_mut().push(screen);
    finish(Some(&baseline), &with_screen);

    // The starter frame keeps its id while the run fills it.
    let starter = EditorState::starter();
    let blank = RunBaseline::capture(&starter);
    let mut filled = starter.clone();
    let root = op_editor_core::PenNodeExt::id_str(&filled.active_children()[0]).to_string();
    insert(&mut filled, &root, text("t", "Hello", 16, "#111827"));
    finish(Some(&blank), &filled);

    // No baseline: today's behaviour, for a document the run did modify.
    let mut modified = existing.clone();
    hero_edit(&mut modified);
    let whole = finish(None, &modified);
    assert_ne!(
        whole,
        canonical(&modified),
        "the whole-page finalize does restyle the page"
    );
}
