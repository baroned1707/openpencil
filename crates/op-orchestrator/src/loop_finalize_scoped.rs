//! The loop finalizer for a run that MODIFIED an existing design — split from
//! `loop_finalize.rs`. [`apply`] and the recorder (`loop_finalize_record`) call
//! the same phase functions below in the same order, so the MCP host replay
//! reproduces this result command for command.
//!
//! What runs, and why it is a subset of the whole-page finalizer:
//!
//! 1. **Direct semantic passes** (roles, post-pass, promote, tree heuristics,
//!    token binding, surface discipline) run over a CLONE of the whole page,
//!    then only the touched sections are copied back by id. They need the
//!    whole page to judge (role inference reads a section's place among its
//!    siblings, the dominant accent reads every section), but they may only
//!    WRITE where the run worked.
//! 2. **App-state hoist** over the touched sections only.
//! 3. **Image slots**: an empty image-fill frame in a touched section becomes
//!    a real image node, because the image-enrichment step only fills those.
//!    It is a structural fix to content the run wrote, not a judgement.
//! 4. **Contract sweep** (text contrast, then geometry) over EVERY top-level
//!    root, always. It only repairs what the resolved geometry proves broken,
//!    and an empty scope never switches it off (see `repair_scope`).
//! 5. **Text fill**, again over a clone with the touched sections copied back
//!    (a text's fill is judged against the backgrounds of its ancestors).
//! 6. **Palette seed**: it only adds tokens the document references but never
//!    defined, so it cannot restyle anything.
//!
//! Not run: the loop prelude and the whole-root cleanup driver
//! (`cleanup::finalize_design`), which carry the document-level passes (page
//! gap, app-shell reshape, theme-variable polarity, duplicate-root dedupe,
//! shared chrome and navigation wiring) and per-root intent passes that treat
//! the page as one unit. They are for a run that built a page.

use super::*;
use crate::run_scope::SectionScope;
use op_editor_core::walkers::{find_node, find_node_mut};

/// Cap on ids named in the ledger line.
const NOTE_IDS: usize = 5;

/// Finalize `state` over `scope`'s sections.
pub(super) fn apply(state: &mut EditorState, scope: &SectionScope) -> RepairSummary {
    let mut summary = RepairSummary::default();
    if state.active_children().is_empty() {
        return summary;
    }
    direct_passes(state, scope, &mut summary);
    app_state_hoist(state, scope);
    {
        let mut sink = StateDocSink { state: &mut *state };
        image_slots(&mut sink, scope);
        contract_sweep(&mut sink, &mut summary);
    }
    text_fill(state, scope);
    crate::loop_finalize_palette::seed_referenced_palette_tokens(state);
    summary.note(scope_note(scope));
    summary
}

/// Phase 1. Returns nothing: the canvas width the whole-page path hands to its
/// cleanup stage is not needed (that stage does not run here).
pub(super) fn direct_passes(
    state: &mut EditorState,
    scope: &SectionScope,
    summary: &mut RepairSummary,
) {
    if scope.is_empty() {
        return;
    }
    let mut whole_page = state.clone();
    run_loop_finalize_direct_passes(&mut whole_page, summary);
    copy_sections(whole_page.active_children(), state, scope);
}

/// Phase 2: hoist node-level `state` out of the touched sections.
pub(super) fn app_state_hoist(
    state: &mut EditorState,
    scope: &SectionScope,
) -> Option<EditorCommand> {
    if scope.is_empty() {
        return None;
    }
    let mut sections: Vec<PenNode> = scope
        .touched()
        .iter()
        .filter_map(|id| find_node(state.active_children(), &NodeId::new(id.clone())).cloned())
        .collect();
    let hoist =
        op_editor_core::hoist_app_state(&mut sections, op_editor_core::UNPLANNED_APP_STATE_IDX);
    let hoists_something = matches!(
        &hoist,
        EditorCommand::MergeAppState { state: hoisted, .. } if !hoisted.is_empty()
    );
    if !hoists_something {
        return None;
    }
    copy_sections(&sections, state, scope);
    state.apply(hoist.clone()).then_some(hoist)
}

/// Phase 3: empty image-fill slots in the touched sections become image nodes.
pub(super) fn image_slots(sink: &mut dyn crate::types::DocSink, scope: &SectionScope) {
    for id in scope.touched() {
        crate::cleanup::cleanup_image_slots::materialize_empty_image_fill_slots(sink, id);
    }
}

/// Phase 4: the contract-tier sweep over every top-level root.
pub(super) fn contract_sweep(sink: &mut dyn crate::types::DocSink, summary: &mut RepairSummary) {
    let roots: Vec<String> = sink
        .state()
        .active_children()
        .iter()
        .map(|node| node.id_str().to_string())
        .collect();
    let root_refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    crate::repair_scope::contract_sweep(sink, &root_refs, summary);
}

/// Phase 5: fill-less text in the touched sections.
pub(super) fn text_fill(state: &mut EditorState, scope: &SectionScope) {
    if scope.is_empty() {
        return;
    }
    let mut whole_page = state.clone();
    run_loop_finalize_text_fill(&mut whole_page);
    copy_sections(whole_page.active_children(), state, scope);
}

/// Replace each touched section of `state` with its counterpart in `from`.
/// A section missing from `from` is left as it is.
fn copy_sections(from: &[PenNode], state: &mut EditorState, scope: &SectionScope) {
    for id in scope.touched() {
        let id = NodeId::new(id.clone());
        let Some(replacement) = find_node(from, &id).cloned() else {
            continue;
        };
        if let Some(slot) = find_node_mut(state.active_children_mut(), &id) {
            *slot = replacement;
        }
    }
}

/// The ledger line. A narrowed scope is a decision, and a reader comparing two
/// credentials must be able to tell it from a gap.
pub(super) fn scope_note(scope: &SectionScope) -> String {
    if scope.is_empty() {
        return "finalize scoped to the run's changes: no section was changed, so no \
                semantic repair ran; contract-tier checks (text contrast, geometry) ran \
                on the full page"
            .to_string();
    }
    let shown: Vec<&str> = scope
        .touched()
        .iter()
        .take(NOTE_IDS)
        .map(String::as_str)
        .collect();
    let more = scope.touched().len().saturating_sub(NOTE_IDS);
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!(
        "finalize scoped to the {} section(s) the run changed ({}{tail}): semantic, token, \
         image-slot and text-fill repairs ran there only, and document-level and per-root \
         cleanup passes (page gap, app shell, theme polarity, chrome dedupe) were skipped; \
         contract-tier checks (text contrast, geometry) ran on the full page",
        scope.touched().len(),
        shown.join(", ")
    )
}
