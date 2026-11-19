//! Replay a scripted transcript through the headless executor and score the
//! result. No model, no clock, no randomness: the same fixture yields the
//! same document bytes and the same numbers.
//!
//! The steps run through [`HeadlessExecutor`], i.e. the production
//! transport-free apply path (`execute_agent_tool_with_root_seed_guard`), then
//! `finalize()` runs the loop-end repairs exactly as a real design run ends.
//! `get_screenshot` is counted and skipped (a real raster is machine-specific
//! and the metrics read the document, not pixels).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use op_ai::chat_provider::ChatToolExecutor;
use op_chat_agent::design_agent_tools::with_font_catalog_file;
use op_chat_agent::design_eval_metrics::design_eval_metrics;
use op_editor_core::EditorState;
use serde_json::{json, Value};

use super::{read_text, EvalError, Expect, Fixture, StepFile};
use crate::loop_mode::HeadlessExecutor;

/// What replaying one fixture produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Replayed {
    /// Every metric by its camelCase name: the document metrics plus the
    /// replay counters (`batchCount`, `screenshotCount`, `replayErrors`,
    /// `requiredTextMissing`, `outsideScopeEdits`, `outsideScopeChanges`,
    /// `expectationFailures`).
    pub metrics: BTreeMap<String, u32>,
    /// The finalized document, pretty-printed (the determinism witness).
    pub doc_json: String,
    /// Why each failed step failed, and which expectations were not met.
    pub problems: Vec<String>,
}

/// Replay `fixture` from its seed (or an empty document) and score it.
pub(crate) fn replay(fixture: &Fixture) -> Result<Replayed, EvalError> {
    // Text measurement resolves the bundled fonts; register them like main().
    op_host_native::bundled_design_fonts::register();
    // The replay must not read the user's cached Google Fonts catalog.
    with_font_catalog_file(None, || replay_steps(fixture))
}

fn replay_steps(fixture: &Fixture) -> Result<Replayed, EvalError> {
    let initial = match fixture.file.seed.as_deref() {
        Some(relative) => load_seed(fixture, relative)?,
        None => EditorState::new(),
    };
    let seed_doc = serde_json::to_value(&initial.doc).unwrap_or(Value::Null);
    let state = Arc::new(Mutex::new(initial));
    let executor = HeadlessExecutor::with_prompt(state.clone(), false, &fixture.file.prompt);

    let mut problems = Vec::new();
    let (mut batches, mut screenshots, mut errors) = (0u32, 0u32, 0u32);
    for (index, step) in fixture.file.steps.iter().enumerate() {
        if step.tool == "get_screenshot" {
            screenshots += 1;
            continue;
        }
        if step.tool == "batch_design" {
            batches += 1;
        }
        let args = step_args(fixture, index, step)?;
        let result = executor.execute(&step.tool, &args);
        if result.is_error {
            errors += 1;
            let reason: String = result.content.chars().take(240).collect();
            problems.push(format!("step {index} {}: {reason}", step.tool));
        }
    }
    // What the scripted calls alone changed, before the loop-end repairs.
    let edited = state.lock().map_or(Value::Null, |guard| {
        serde_json::to_value(&guard.doc).unwrap_or(Value::Null)
    });
    let _ = executor.finalize();

    let guard = state.lock().expect("EditorState mutex");
    let doc = serde_json::to_value(&guard.doc).unwrap_or(Value::Null);
    let doc_json = serde_json::to_string_pretty(&guard.doc).unwrap_or_default();
    let mut metrics = flatten(&design_eval_metrics(&guard));
    let required_missing = missing_required_text(&fixture.file.expect, &doc);
    let outside = |after: &Value| {
        fixture
            .file
            .expect
            .scope_root_id
            .as_deref()
            .map_or(0, |scope| outside_scope_changes(&seed_doc, after, scope))
    };
    for (name, value) in [
        ("batchCount", batches),
        ("screenshotCount", screenshots),
        ("replayErrors", errors),
        ("requiredTextMissing", count(required_missing.len())),
        ("outsideScopeEdits", outside(&edited)),
        ("outsideScopeChanges", outside(&doc)),
    ] {
        metrics.insert(name.to_string(), value);
    }
    for text in required_missing {
        problems.push(format!("required text not found: {text:?}"));
    }
    let violated = bound_violations(&fixture.file.expect, &metrics);
    metrics.insert("expectationFailures".into(), count(violated.len()));
    problems.extend(violated);
    Ok(Replayed {
        metrics,
        doc_json,
        problems,
    })
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn flatten(metrics: &impl serde::Serialize) -> BTreeMap<String, u32> {
    let Ok(Value::Object(fields)) = serde_json::to_value(metrics) else {
        return BTreeMap::new();
    };
    fields
        .into_iter()
        .filter_map(|(name, value)| Some((name, u32::try_from(value.as_u64()?).ok()?)))
        .collect()
}

fn load_seed(fixture: &Fixture, relative: &str) -> Result<EditorState, EvalError> {
    let text = read_text(&fixture.dir.join(relative))?;
    let loaded = jian_ops_schema::load_str(&text).map_err(|error| EvalError::Seed {
        case: fixture.id.clone(),
        reason: error.to_string(),
    })?;
    Ok(EditorState::from_document(loaded.value))
}

/// The JSON a step sends: its `args`, or `{ "script": <file> }`.
fn step_args(fixture: &Fixture, index: usize, step: &StepFile) -> Result<String, EvalError> {
    match (&step.args, &step.script_file) {
        (Some(args), None) => Ok(args.to_string()),
        (None, Some(file)) => {
            let script = read_text(&fixture.dir.join(file))?;
            Ok(json!({ "script": script }).to_string())
        }
        _ => Err(EvalError::BadStep {
            case: fixture.id.clone(),
            index,
        }),
    }
}

/// Every `type: "text"` string in a serialized document (all pages).
fn document_texts(node: &Value, out: &mut Vec<String>) {
    match node {
        Value::Object(fields) => {
            if fields.get("type").and_then(Value::as_str) == Some("text") {
                match fields.get("content") {
                    Some(Value::String(text)) => out.push(text.clone()),
                    Some(Value::Array(segments)) => out.push(
                        segments
                            .iter()
                            .filter_map(|segment| segment.get("text").and_then(Value::as_str))
                            .collect(),
                    ),
                    _ => {}
                }
            }
            fields.values().for_each(|child| document_texts(child, out));
        }
        Value::Array(items) => items.iter().for_each(|child| document_texts(child, out)),
        _ => {}
    }
}

/// `requiredText` entries that no text node contains (case-insensitive).
pub(super) fn missing_required_text(expect: &Expect, doc: &Value) -> Vec<String> {
    let mut texts = Vec::new();
    document_texts(doc, &mut texts);
    let lowered: Vec<String> = texts.iter().map(|text| text.to_lowercase()).collect();
    expect
        .required_text
        .iter()
        .filter(|needle| {
            let needle = needle.to_lowercase();
            !lowered.iter().any(|text| text.contains(&needle))
        })
        .cloned()
        .collect()
}

/// `min` / `max` bounds that `metrics` violates (an unknown name counts).
pub(super) fn bound_violations(expect: &Expect, metrics: &BTreeMap<String, u32>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, bound) in &expect.min {
        match metrics.get(name) {
            Some(value) if value >= bound => {}
            other => out.push(format!("{name} = {other:?}, expected at least {bound}")),
        }
    }
    for (name, bound) in &expect.max {
        match metrics.get(name) {
            Some(value) if value <= bound => {}
            other => out.push(format!("{name} = {other:?}, expected at most {bound}")),
        }
    }
    out
}

/// Nodes outside the `scope` subtree whose own properties (children aside)
/// differ between the seed and the result, counting added and removed nodes.
pub(super) fn outside_scope_changes(seed: &Value, result: &Value, scope: &str) -> u32 {
    let before = flatten_nodes(seed);
    let after = flatten_nodes(result);
    let inside: BTreeSet<&String> = [&before, &after]
        .iter()
        .flat_map(|nodes| subtree_ids(nodes, scope))
        .collect();
    let ids: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    count(
        ids.into_iter()
            .filter(|id| !inside.contains(*id) && before.get(*id) != after.get(*id))
            .count(),
    )
}

/// One node as the differ sees it: its parent id and its JSON without children.
type FlatNode = (Option<String>, Value);

fn flatten_nodes(doc: &Value) -> BTreeMap<String, FlatNode> {
    fn walk(node: &Value, parent: Option<&str>, out: &mut BTreeMap<String, FlatNode>) {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            return;
        };
        let mut own = node.clone();
        if let Some(fields) = own.as_object_mut() {
            fields.remove("children");
        }
        out.insert(id.to_string(), (parent.map(str::to_string), own));
        for child in node
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            walk(child, Some(id), out);
        }
    }
    let mut out = BTreeMap::new();
    let pages = doc.get("pages").and_then(Value::as_array);
    let roots = pages
        .into_iter()
        .flatten()
        .filter_map(|page| page.get("children"))
        .chain(doc.get("children"));
    for list in roots {
        for node in list.as_array().into_iter().flatten() {
            walk(node, None, &mut out);
        }
    }
    out
}

/// `scope` and every descendant of it in `nodes`.
fn subtree_ids<'a>(nodes: &'a BTreeMap<String, FlatNode>, scope: &str) -> Vec<&'a String> {
    let mut inside: Vec<&String> = nodes.keys().filter(|id| id.as_str() == scope).collect();
    loop {
        let before = inside.len();
        for (id, (parent, _)) in nodes {
            let child_of_inside = parent
                .as_deref()
                .is_some_and(|parent| inside.iter().any(|known| known.as_str() == parent));
            if child_of_inside && !inside.contains(&id) {
                inside.push(id);
            }
        }
        if inside.len() == before {
            return inside;
        }
    }
}
