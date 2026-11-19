//! What one agent run touched — the scope of a modify-run finalize.
//!
//! The loop finalizer (`loop_finalize`) was written for a run that BUILDS a
//! page, so it repairs the whole page. Run on "make the hero bolder" it gave
//! `nav`, `features` and three cards a `role`, and the page frame a `gap`,
//! none of which the run had touched (eval case `09-modify-hero-bolder`
//! counted six such nodes).
//!
//! This module answers "which sections did the run change?" so the finalizer
//! can confine its judgement to them. It is pure: it reads a pre-run
//! [`RunBaseline`] and the current document, and writes nothing.
//!
//! ## Units
//!
//! The unit of scope is a SECTION: a child of the page-root wrapper when the
//! page is one wrapper frame (the same test `loop_finalize` uses), otherwise a
//! top-level root (a multi-screen document: one screen per unit).
//!
//! ## How "touched" is decided
//!
//! By CONTENT, not by id. A modify run edits with `U()`, which keeps ids, so an
//! id diff sees nothing (`agent_reveals` diffs ids and only sees inserts). A
//! unit is touched when its serialized subtree differs from the same unit in
//! the baseline, or when the unit is new. That one comparison also covers a
//! child added, removed, moved or reordered inside the unit, and a node moved
//! between two units touches both.
//!
//! ## When the answer is [`RunScope::Whole`]
//!
//! Scoping is an optimisation for a run that modified an existing design. It
//! is refused (the finalizer runs exactly as it always did) whenever the run
//! built the design, or when the baseline cannot prove otherwise:
//!
//! - no baseline, or the run switched page;
//! - a top-level frame appeared, disappeared or moved in the stack;
//! - the baseline held no content (a blank canvas, the starter frame, an
//!   empty scaffold): a fresh design keeps its root id when it reuses that
//!   frame, so an id check alone would call it a modify run;
//! - the page wrapper's own fields changed (page fill, width, ...);
//! - the page wrapper's surviving sections were reordered;
//! - no unit that had content is unchanged, i.e. the run rewrote everything.
//!
//! An EMPTY scope is a real answer (nothing in the forest changed, e.g. only a
//! variable was set): it is `Sections` with no touched units, and the finalizer
//! still runs its contract-tier sweep. It never means "skip".

use jian_ops_schema::node::PenNode;
use op_editor_core::walkers::find_node;
use op_editor_core::{EditorState, NodeId, PenNodeExt, RunBaseline};
use serde_json::Value;
use std::collections::HashMap;

/// The sections a finalize may change, by id, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionScope {
    touched: Vec<String>,
}

impl SectionScope {
    /// The touched section ids, in document order.
    pub fn touched(&self) -> &[String] {
        &self.touched
    }

    pub fn is_empty(&self) -> bool {
        self.touched.is_empty()
    }
}

/// How much of the page a finalize may judge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunScope {
    /// The whole page: today's finalize, byte for byte. Every new design, and
    /// every run the baseline cannot prove to be a modification.
    Whole,
    /// Only these sections (possibly none); contract-tier checks still cover
    /// the whole page.
    Sections(SectionScope),
}

/// Why `finalize_design` could not turn `root_ids` into a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunScopeError {
    /// These ids name no node on the active page.
    UnknownIds(Vec<String>),
    /// The list was empty.
    NoIds,
}

impl std::fmt::Display for RunScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunScopeError::UnknownIds(ids) => write!(
                f,
                "root_ids names no node on the active page: {}",
                ids.join(", ")
            ),
            RunScopeError::NoIds => f.write_str("root_ids must name at least one node"),
        }
    }
}

impl std::error::Error for RunScopeError {}

impl RunScope {
    /// The scope of the run that started at `baseline` and ended at `state`.
    /// `None` (no baseline was taken) is the safe default: the whole page.
    pub fn since(baseline: Option<&RunBaseline>, state: &EditorState) -> RunScope {
        let Some(baseline) = baseline else {
            return RunScope::Whole;
        };
        if baseline.page_index() != state.ui.active_page_index {
            return RunScope::Whole;
        }
        compute_scope(baseline.roots(), state.active_children())
    }

    /// The scope a caller names with `root_ids`: each id is mapped to the
    /// section that contains it (an id deeper than a section finalizes that
    /// section). Naming the page wrapper, or every section, is the whole page.
    pub fn from_root_ids(state: &EditorState, ids: &[String]) -> Result<RunScope, RunScopeError> {
        if ids.is_empty() {
            return Err(RunScopeError::NoIds);
        }
        let forest = state.active_children();
        let unknown: Vec<String> = ids
            .iter()
            .filter(|id| find_node(forest, &NodeId::new((*id).clone())).is_none())
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(RunScopeError::UnknownIds(unknown));
        }
        let (wrapper, units) = section_units(forest);
        let mut touched = Vec::new();
        for unit in units {
            let contains = |id: &String| {
                unit.id_str() == id
                    || unit
                        .children()
                        .is_some_and(|c| find_node(c, &NodeId::new(id.clone())).is_some())
            };
            if ids.iter().any(contains) {
                touched.push(unit.id_str().to_string());
            }
        }
        let names_wrapper = wrapper.is_some_and(|w| ids.iter().any(|id| id == w.id_str()));
        if names_wrapper || touched.len() == section_units(forest).1.len() {
            return Ok(RunScope::Whole);
        }
        Ok(RunScope::Sections(SectionScope { touched }))
    }

    /// The touched section ids, or `None` for the whole page.
    pub fn touched_sections(&self) -> Option<&[String]> {
        match self {
            RunScope::Whole => None,
            RunScope::Sections(scope) => Some(scope.touched()),
        }
    }
}

/// The page wrapper (when the page is one wrapper frame) and the sections.
pub(crate) fn section_units(forest: &[PenNode]) -> (Option<&PenNode>, &[PenNode]) {
    match forest {
        [only] if crate::loop_finalize::is_page_root_wrapper(only) => {
            (Some(only), only.children().map_or(&[][..], Vec::as_slice))
        }
        _ => (None, forest),
    }
}

/// The pure scope computation: `before` and `after` are the active page's
/// top-level forest at the start and at the end of the run.
pub(crate) fn compute_scope(before: &[PenNode], after: &[PenNode]) -> RunScope {
    let has_content = |roots: &[PenNode]| {
        roots
            .iter()
            .any(|root| root.children().is_some_and(|c| !c.is_empty()))
    };
    if !has_content(before) {
        return RunScope::Whole;
    }
    let ids = |roots: &[PenNode]| -> Vec<String> {
        roots.iter().map(|r| r.id_str().to_string()).collect()
    };
    if ids(before) != ids(after) {
        return RunScope::Whole;
    }
    let (before_wrapper, before_units) = section_units(before);
    let (after_wrapper, after_units) = section_units(after);
    if before_wrapper.is_some() != after_wrapper.is_some() {
        return RunScope::Whole;
    }
    if let (Some(old), Some(new)) = (before_wrapper, after_wrapper) {
        if wrapper_changed(old, new) || sections_reordered(before_units, after_units) {
            return RunScope::Whole;
        }
    }
    let old_units: HashMap<&str, Value> = before_units
        .iter()
        .map(|unit| (unit.id_str(), to_value(unit)))
        .collect();
    let mut touched = Vec::new();
    let mut anchored = false;
    for unit in after_units {
        let unchanged = old_units
            .get(unit.id_str())
            .is_some_and(|old| *old == to_value(unit));
        if !unchanged {
            touched.push(unit.id_str().to_string());
        } else if unit.children().is_some_and(|c| !c.is_empty()) {
            anchored = true;
        }
    }
    if !anchored {
        return RunScope::Whole;
    }
    RunScope::Sections(SectionScope { touched })
}

/// The node as JSON, with an empty or null `children` dropped everywhere:
/// "no children" and "an empty list" are the same content, and an editor write
/// that re-serializes one into the other (an empty image-slot rectangle gains
/// `children: []` on any insert) must not make a section look edited.
fn to_value(node: &PenNode) -> Value {
    let mut value = serde_json::to_value(node).unwrap_or(Value::Null);
    drop_empty_children(&mut value);
    value
}

fn drop_empty_children(value: &mut Value) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    let empty = match fields.get("children") {
        Some(Value::Array(items)) => items.is_empty(),
        Some(Value::Null) => true,
        _ => false,
    };
    if empty {
        fields.remove("children");
    }
    if let Some(Value::Array(items)) = fields.get_mut("children") {
        items.iter_mut().for_each(drop_empty_children);
    }
}

/// The wrapper's own fields (everything but its children) differ.
fn wrapper_changed(old: &PenNode, new: &PenNode) -> bool {
    let own = |node: &PenNode| {
        let mut value = to_value(node);
        if let Some(fields) = value.as_object_mut() {
            fields.remove("children");
        }
        value
    };
    own(old) != own(new)
}

/// The sections present both before and after are no longer in the same order.
fn sections_reordered(before: &[PenNode], after: &[PenNode]) -> bool {
    let survivors = |from: &[PenNode], other: &[PenNode]| -> Vec<String> {
        from.iter()
            .filter(|node| other.iter().any(|o| o.id_str() == node.id_str()))
            .map(|node| node.id_str().to_string())
            .collect()
    };
    survivors(before, after) != survivors(after, before)
}

#[cfg(test)]
#[path = "run_scope_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "run_scope_tests.rs"]
mod tests;
