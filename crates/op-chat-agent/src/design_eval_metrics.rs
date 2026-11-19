//! Offline design-quality metrics: ONE pure function that scores a finished
//! document, used by the eval harness in `op-smoke` (no model, no clock, no
//! randomness: the same document always yields the same numbers).
//!
//! What it measures
//! - Issue counts (`layoutIssues`, `iconIssues`, `contrastIssues`,
//!   `contrastUncertain`, `fontIssues`, `structureIssues`, `navIssues`): taken from
//!   [`post_write_diagnostics`] itself with no caps, so the eval counts exactly
//!   what a design write reports to the model. The collectors keep their own
//!   internal limits (contrast 64, geometry / icon / font / shell lists 12 or
//!   fewer, ring and header rows 4); a page that hits one of them is
//!   under-counted, never over-counted.
//! - Content: placeholder text and repeated-row text (see [`PLACEHOLDER_WHOLE_TEXTS`]
//!   and `add_row_stats`).
//! - Token conformance: how many solid fills (including text colours) are
//!   bound to a design variable (`$name`).
//! - Report-only extras: tap targets under 24 x 24 on interactive-looking
//!   nodes, distinct font sizes per section, node / screen / page counts.
//!
//! Every collector reads the ACTIVE page, so the function clones the state once
//! and walks every page by moving the active index; the counts are summed.
//! Integer fields only (shares are per-mille), so serialized output never
//! depends on float formatting. Field order is the declaration order below.

use std::collections::{BTreeMap, BTreeSet};

use op_editor_core::EditorState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::design_agent_diagnostics::resolved_sizes;
use crate::design_agent_tools::{post_write_diagnostics, with_font_catalog_file, PostWriteExtras};

/// A text that is placeholder copy when it is the WHOLE trimmed text
/// (compared case-insensitively). `lorem ipsum` is matched anywhere in a text
/// instead (see [`is_placeholder_text`]).
pub const PLACEHOLDER_WHOLE_TEXTS: [&str; 4] = ["placeholder", "text here", "title", "description"];

/// Substring that marks filler text wherever it appears (case-insensitive).
pub const PLACEHOLDER_SUBSTRING: &str = "lorem ipsum";

/// A tap target smaller than this on either side is counted.
pub const MIN_TAP_TARGET_PX: f64 = 24.0;

/// Name tokens that make a node look interactive when it carries no events.
const INTERACTIVE_NAME_TOKENS: [&str; 11] = [
    "button", "btn", "tab", "toggle", "checkbox", "switch", "chip", "cta", "radio", "link", "fab",
];

/// The metrics of one document. Gating fields are the six issue counts,
/// `placeholder_text` and `content_coverage_permille`; everything else is
/// report-only (the gate lives in `op-smoke`, not here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignEvalMetrics {
    pub layout_issues: u32,
    pub icon_issues: u32,
    pub contrast_issues: u32,
    pub contrast_uncertain: u32,
    pub font_issues: u32,
    pub structure_issues: u32,
    /// Multi-screen apps: nav tabs not wired to switch screens (report-only).
    pub nav_issues: u32,
    /// Text nodes whose text is placeholder copy.
    pub placeholder_text: u32,
    /// Text rows that repeat another row's text in the same position of a run
    /// of like siblings (3 or more).
    pub duplicate_row_text: u32,
    /// Text rows inside such runs (the denominator of the coverage share).
    pub repeated_row_texts: u32,
    /// Share of repeated-structure text rows with their own text, per mille
    /// (1000 when there are no repeated structures).
    pub content_coverage_permille: u32,
    /// Solid fills (text colours included) bound to a variable (`$name`).
    pub token_bound_fills: u32,
    /// All solid fills, text colours included.
    pub solid_fills: u32,
    /// `token_bound_fills / solid_fills` per mille (1000 when there are none).
    pub token_conformance_permille: u32,
    /// Interactive-looking nodes smaller than 24 x 24 on either side.
    pub tap_targets_under_24: u32,
    /// Interactive-looking nodes (the denominator of the line above).
    pub interactive_nodes: u32,
    /// Most distinct font sizes in any one section of a screen.
    pub type_sizes_max_per_section: u32,
    pub node_count: u32,
    /// Top-level screen-like frames over all pages (`list_screen_candidates`).
    pub screen_count: u32,
    pub page_count: u32,
}

/// Make font feedback machine-independent: the eval pretends the host
/// enumerated a fixed system-font list (so unknown families CAN be judged; a
/// headless state with no system fonts and no catalog never flags any) and the
/// caller reads no Google Fonts catalog from the user's config directory
/// (see [`design_eval_metrics`]).
pub fn pin_eval_font_environment(state: &mut EditorState) {
    state.editor_ui.system_fonts_loaded = true;
    state.editor_ui.system_font_families = std::sync::Arc::new(
        op_editor_ui::font_catalog::FALLBACK_SYSTEM_FONTS
            .iter()
            .map(|family| family.to_string())
            .collect(),
    );
}

/// Score `state` over every page. Pure: `state` is cloned, never mutated, and
/// the font environment is pinned (see [`pin_eval_font_environment`]).
pub fn design_eval_metrics(state: &EditorState) -> DesignEvalMetrics {
    with_font_catalog_file(None, || score_pages(state))
}

fn score_pages(state: &EditorState) -> DesignEvalMetrics {
    let mut scratch = state.clone();
    pin_eval_font_environment(&mut scratch);
    let mut out = DesignEvalMetrics {
        page_count: count(scratch.page_count()),
        ..DesignEvalMetrics::default()
    };
    let mut row_stats = RowStats::default();
    for page_index in 0..scratch.page_count() {
        scratch.ui.active_page_index = page_index;
        add_issue_counts(&mut out, &scratch);
        out.screen_count +=
            count(op_orchestrator::unfilled_screens::list_screen_candidates(&scratch).len());
        let sizes = resolved_sizes(&scratch);
        let mut page = PageWalk::default();
        for root in scratch.active_children() {
            // `PenNode` always serializes; a failure would be a schema bug and
            // is scored as an empty node rather than aborting the eval.
            let value = serde_json::to_value(root).unwrap_or(Value::Null);
            page.walk(&value, &sizes, &mut row_stats);
            page.sections(&value);
        }
        out.placeholder_text += page.placeholder_text;
        out.solid_fills += page.solid_fills;
        out.token_bound_fills += page.token_bound_fills;
        out.interactive_nodes += page.interactive_nodes;
        out.tap_targets_under_24 += page.tap_targets_under_24;
        out.node_count += page.node_count;
        out.type_sizes_max_per_section = out
            .type_sizes_max_per_section
            .max(page.type_sizes_max_per_section);
    }
    out.duplicate_row_text = row_stats.duplicate;
    out.repeated_row_texts = row_stats.total;
    out.content_coverage_permille =
        permille(row_stats.total - row_stats.duplicate, row_stats.total);
    out.token_conformance_permille = permille(out.token_bound_fills, out.solid_fills);
    out
}

/// Add the active page's issue counts, read from the very function a design
/// write calls (no caps), so the two can never disagree.
fn add_issue_counts(out: &mut DesignEvalMetrics, state: &EditorState) {
    let report = post_write_diagnostics(state, PostWriteExtras::default(), None);
    let len = |key: &str| {
        count(
            report
                .get(key)
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        )
    };
    out.layout_issues += len("layoutIssues");
    out.icon_issues += len("iconIssues");
    out.contrast_issues += len("contrastIssues");
    out.contrast_uncertain += len("contrastUncertain");
    out.font_issues += len("fontIssues");
    out.structure_issues += len("structureIssues");
    out.nav_issues += len("navIssues");
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// `part / whole` per mille, rounded down; 1000 when `whole` is 0.
fn permille(part: u32, whole: u32) -> u32 {
    if whole == 0 {
        1000
    } else {
        (u64::from(part) * 1000 / u64::from(whole)) as u32
    }
}

/// True for filler copy: `lorem ipsum` anywhere, or one of
/// [`PLACEHOLDER_WHOLE_TEXTS`] as the whole text. A `text_input`'s
/// `placeholder` property is real UI copy and is never looked at.
pub fn is_placeholder_text(text: &str) -> bool {
    let lowered = text.trim().to_lowercase();
    lowered.contains(PLACEHOLDER_SUBSTRING) || PLACEHOLDER_WHOLE_TEXTS.contains(&lowered.as_str())
}

#[derive(Default)]
struct RowStats {
    total: u32,
    duplicate: u32,
}

/// Per-page tallies of the JSON walk.
#[derive(Default)]
struct PageWalk {
    node_count: u32,
    placeholder_text: u32,
    solid_fills: u32,
    token_bound_fills: u32,
    interactive_nodes: u32,
    tap_targets_under_24: u32,
    type_sizes_max_per_section: u32,
}

impl PageWalk {
    fn walk(
        &mut self,
        node: &Value,
        sizes: &std::collections::HashMap<String, (f64, f64)>,
        rows: &mut RowStats,
    ) {
        self.node_count += 1;
        let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
        if node_type == "text" && is_placeholder_text(&text_of(node)) {
            self.placeholder_text += 1;
        }
        for fill in node
            .get("fill")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if fill.get("type").and_then(Value::as_str) == Some("solid") {
                self.solid_fills += 1;
                let color = fill.get("color").and_then(Value::as_str).unwrap_or("");
                if color.starts_with('$') {
                    self.token_bound_fills += 1;
                }
            }
        }
        if looks_interactive(node, node_type) {
            self.interactive_nodes += 1;
            let id = node.get("id").and_then(Value::as_str).unwrap_or("");
            if let Some((width, height)) = sizes.get(id) {
                if *width < MIN_TAP_TARGET_PX || *height < MIN_TAP_TARGET_PX {
                    self.tap_targets_under_24 += 1;
                }
            }
        }
        let children = children_of(node);
        add_row_stats(children, rows);
        for child in children {
            self.walk(child, sizes, rows);
        }
    }

    /// Distinct font sizes per section: the container children of a top-level
    /// node (or the node itself when it has none).
    fn sections(&mut self, root: &Value) {
        let containers: Vec<&Value> = children_of(root)
            .iter()
            .filter(|child| child.get("children").is_some())
            .collect();
        let sections: Vec<&Value> = if containers.is_empty() {
            vec![root]
        } else {
            containers
        };
        for section in sections {
            let mut sizes = BTreeSet::new();
            collect_font_sizes(section, &mut sizes);
            self.type_sizes_max_per_section =
                self.type_sizes_max_per_section.max(count(sizes.len()));
        }
    }
}

fn children_of(node: &Value) -> &[Value] {
    node.get("children")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// The text of a text node: plain content, or the styled segments joined.
fn text_of(node: &Value) -> String {
    match node.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(segments)) => segments
            .iter()
            .filter_map(|segment| segment.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

fn collect_font_sizes(node: &Value, out: &mut BTreeSet<u64>) {
    if node.get("type").and_then(Value::as_str) == Some("text") {
        if let Some(size) = node.get("fontSize").and_then(Value::as_f64) {
            out.insert(size.to_bits());
        }
    }
    for child in children_of(node) {
        collect_font_sizes(child, out);
    }
}

/// Events, an interactive role, a `text_input`, or a name that says so.
fn looks_interactive(node: &Value, node_type: &str) -> bool {
    if node_type == "text_input" || node.get("events").is_some_and(|events| !events.is_null()) {
        return true;
    }
    let role = node.get("role").and_then(Value::as_str).unwrap_or("");
    let name = node.get("name").and_then(Value::as_str).unwrap_or("");
    [role, name].iter().any(|label| {
        label
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| INTERACTIVE_NAME_TOKENS.contains(&token))
    })
}

/// Shape of a subtree: node types only, nested. Two siblings with the same
/// shape are "like" siblings (a list row, a card).
fn shape_of(node: &Value) -> String {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("?");
    let inner: Vec<String> = children_of(node).iter().map(shape_of).collect();
    format!("{node_type}({})", inner.join(","))
}

fn texts_in(node: &Value, out: &mut Vec<String>) {
    if node.get("type").and_then(Value::as_str) == Some("text") {
        out.push(text_of(node).trim().to_string());
    }
    for child in children_of(node) {
        texts_in(child, out);
    }
}

/// For every run of 3 or more like siblings, compare the text in each text
/// position across the run: a position where `n` rows show `d` distinct texts
/// adds `n` rows and `n - d` duplicates.
fn add_row_stats(children: &[Value], rows: &mut RowStats) {
    let mut runs: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for child in children {
        runs.entry(shape_of(child)).or_default().push(child);
    }
    for run in runs.values().filter(|run| run.len() >= 3) {
        let texts: Vec<Vec<String>> = run
            .iter()
            .map(|child| {
                let mut found = Vec::new();
                texts_in(child, &mut found);
                found
            })
            .collect();
        let positions = texts.first().map_or(0, Vec::len);
        for position in 0..positions {
            let column: Vec<&String> = texts.iter().filter_map(|row| row.get(position)).collect();
            let distinct: BTreeSet<&String> = column.iter().copied().collect();
            rows.total += count(column.len());
            rows.duplicate += count(column.len() - distinct.len());
        }
    }
}

#[cfg(test)]
#[path = "design_eval_metrics_tests.rs"]
mod tests;
