//! What the design loop's model has already been told about its writes.
//!
//! [`FeedbackLedger`] is the per-run memory behind two shaping rules of
//! [`crate::design_run_executor::DesignRunExecutor`]:
//!
//! - **Report each diagnostic once per run.** A line the model has already
//!   been shown, and that is still present, is replaced by a `<key>Unchanged`
//!   count; the earlier result with the full line is still in its context.
//!   The memory is per target page, because a write scans only the page it
//!   landed on: another page's lines are not "gone". A line that a later,
//!   uncut write no longer carries is forgotten, so a fixed issue that comes
//!   back is reported again. When the write was cut by a cap
//!   (`diagnosticsTruncated`) a missing line may only be hidden, so nothing is
//!   forgotten on that write.
//! - **Two failed fixes mean re-plan.** The same line in three consecutive
//!   writes adds ONE `replanHint`. The count is taken from the raw lists
//!   (before the repeats are dropped) and fires exactly on the third write,
//!   not on every later one.
//!
//! Lines are matched whole, never by ids parsed out of free-form layout text
//! (`loop_blocker_ledger.rs` explains why that is fragile): the rule can miss
//! a repeat but cannot flag a different issue. The one structured exception is
//! a `contrastIssues` object, matched by its own `nodeId` field for the
//! re-plan count only, because its colours and ratio change with every
//! attempted fix.
//!
//! This is state for what the model SEES. The completion gates (`finalize`,
//! `check_blockers`) recompute from the live document and never read it.

use std::collections::HashMap;

use serde_json::{json, Map, Value};

use crate::design_agent_tools::LIST_KEYS;

/// Consecutive writes carrying the same line before the re-plan hint fires.
const REPLAN_AFTER_WRITES: u32 = 3;

/// Keys whose lines are defects the model is expected to fix. `imageSlots`,
/// `intentQuestions` and `contrastUncertain` stay open on purpose and
/// `variableIssues` is advice, so repeating them says nothing about a failed
/// fix.
const REPLAN_KEYS: [&str; 6] = [
    "layoutIssues",
    "iconIssues",
    "fontIssues",
    "structureIssues",
    "contrastIssues",
    "navIssues",
];

/// Marker the write feedback carries when a cap dropped anything.
const TRUNCATED_KEY: &str = "diagnosticsTruncated";

/// Longest stretch of a repeated line quoted in the re-plan hint.
const HINT_LINE_CHARS: usize = 100;

struct SeenLine {
    /// The entry exactly as the model was shown it.
    value: Value,
    /// What the re-plan count follows across writes (see [`streak_id`]).
    streak_id: String,
    /// Consecutive writes that carried it, this one included.
    streak: u32,
}

#[derive(Default)]
pub(crate) struct FeedbackLedger {
    /// Target page key -> list key -> the lines of the last write there.
    pages: HashMap<String, HashMap<String, Vec<SeenLine>>>,
}

impl FeedbackLedger {
    /// Shape one write result for the model: drop lines it has already seen,
    /// add `<key>Unchanged` counts and, when a line reaches its third write in
    /// a row, one `replanHint`. `page` is the write's page key (see
    /// [`page_key`]). Returns whether `object` changed.
    pub(crate) fn shape(&mut self, page: &str, object: &mut Map<String, Value>) -> bool {
        let truncated = object.get(TRUNCATED_KEY) == Some(&Value::Bool(true));
        let page_lines = self.pages.entry(page.to_string()).or_default();
        let mut changed = false;
        let mut replan: Vec<String> = Vec::new();
        for key in LIST_KEYS {
            let Some(items) = object.get(key).and_then(Value::as_array).cloned() else {
                if !truncated {
                    page_lines.remove(key);
                }
                continue;
            };
            let before = page_lines.remove(key).unwrap_or_default();
            let mut now: Vec<SeenLine> = Vec::with_capacity(items.len());
            let mut fresh: Vec<Value> = Vec::new();
            let mut repeated = 0usize;
            for item in &items {
                if before.iter().any(|seen| seen.value == *item) {
                    repeated += 1;
                } else {
                    fresh.push(item.clone());
                }
                let id = streak_id(key, item);
                let streak = before
                    .iter()
                    .find(|seen| seen.streak_id == id)
                    .map_or(1, |seen| seen.streak + 1);
                if streak == REPLAN_AFTER_WRITES && REPLAN_KEYS.contains(&key) {
                    replan.push(describe(key, item));
                }
                now.push(SeenLine {
                    value: item.clone(),
                    streak_id: id,
                    streak,
                });
            }
            if truncated {
                // A line this write does not show may only be cut off.
                now.extend(
                    before
                        .into_iter()
                        .filter(|seen| !items.contains(&seen.value)),
                );
            }
            page_lines.insert(key.to_string(), now);
            if repeated > 0 {
                if fresh.is_empty() {
                    object.remove(key);
                } else {
                    object.insert(key.to_string(), Value::Array(fresh));
                }
                object.insert(format!("{key}Unchanged"), json!(repeated));
                changed = true;
            }
        }
        if let Some((first, rest)) = replan.split_first() {
            let more = if rest.is_empty() {
                String::new()
            } else {
                format!(" (and {} more)", rest.len())
            };
            object.insert(
                "replanHint".into(),
                json!(format!(
                    "Still open after {REPLAN_AFTER_WRITES} writes in a row: {first}{more}. \
                     Stop patching it: re-read that part with snapshot_layout and rebuild it \
                     with a new plan instead of another small fix."
                )),
            );
            changed = true;
        }
        changed
    }
}

/// The page a `batch_design` call targets under the three spellings the
/// command applier accepts, or `""` for the default (active) page.
pub(crate) fn page_key(args_json: &str) -> String {
    let Ok(args) = serde_json::from_str::<Value>(args_json) else {
        return String::new();
    };
    ["pageId", "page_id", "page"]
        .iter()
        .find_map(|name| args.get(*name).and_then(Value::as_str))
        .map(|page| page.trim().to_string())
        .unwrap_or_default()
}

/// What the re-plan count follows: the whole entry, except that a contrast
/// object follows its `nodeId` (its colours change with each attempted fix).
fn streak_id(key: &str, item: &Value) -> String {
    if key == "contrastIssues" {
        if let Some(node_id) = item.get("nodeId").and_then(Value::as_str) {
            return format!("contrast:{node_id}");
        }
    }
    item.to_string()
}

/// A short quote of the repeated entry for the hint.
fn describe(key: &str, item: &Value) -> String {
    let text = match (key, item.get("nodeId").and_then(Value::as_str)) {
        ("contrastIssues", Some(node_id)) => format!("contrast of node {node_id}"),
        _ => item
            .as_str()
            .map_or_else(|| item.to_string(), str::to_string),
    };
    if text.chars().count() <= HINT_LINE_CHARS {
        return text;
    }
    let cut: String = text.chars().take(HINT_LINE_CHARS - 3).collect();
    format!("{cut}...")
}

#[cfg(test)]
#[path = "design_run_feedback_tests.rs"]
mod tests;
