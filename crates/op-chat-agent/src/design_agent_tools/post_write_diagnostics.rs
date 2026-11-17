//! The feedback a design write hands back to whoever made it.
//!
//! One function, [`post_write_diagnostics`], turns the detect-only quality scans
//! into the JSON keys the in-app design agent has always received after a
//! `batch_design` (`layoutIssues`, `iconIssues`, `fontIssues`,
//! `structureIssues`, `contrastIssues`, `shellsRemaining`, `intentQuestions`, `variableIssues`,
//! `imageSlots`, `navIssues`, plus the `*Hint` sentences). The in-app wrapper
//! (`execute_design_tool_with_root_seed_guard`) and the MCP write path
//! (`op-host-services/src/mcp_serve/write_diagnostics.rs`) both call it, so an
//! external agent sees exactly what the built-in one sees.
//!
//! The in-app caller passes no caps and gets the full, unabbreviated result.
//! The MCP caller passes [`MCP_FEEDBACK_CAPS`]: a bounded number of short
//! lines per key and a byte budget for the whole appendix, because the result
//! rides inside an external agent's context window.

use serde_json::{json, Map, Value};

use super::font_issues::{scan_font_issues, FontKnowledge};
use super::quality_scans::{contrast_hint, ContrastIssue};
use super::*;

/// Facts only the in-app wrapper knows: what its own repair passes just did
/// and the root-seed guard's hint. Empty for the MCP path.
#[derive(Debug, Default)]
pub struct PostWriteExtras {
    /// Appended to `structureIssues` after the scans' own lines.
    pub structure_notes: Vec<String>,
    /// Folded into `layoutHint`.
    pub root_seed_hint: Option<String>,
}

/// Bounds for feedback that is appended to an external agent's tool result.
#[derive(Debug, Clone, Copy)]
pub struct FeedbackCaps {
    /// At most this many items under any one key.
    pub max_items: usize,
    /// Each item (one line) is cut to this many characters.
    pub max_item_chars: usize,
    /// The whole appendix, serialized, stays within this many bytes.
    pub max_total_bytes: usize,
}

/// The caps every external MCP write result is held to.
pub const MCP_FEEDBACK_CAPS: FeedbackCaps = FeedbackCaps {
    max_items: 12,
    max_item_chars: 240,
    max_total_bytes: 4096,
};

/// Keys whose value is a list of items, in the order the envelope lists them.
const LIST_KEYS: [&str; 9] = [
    "layoutIssues",
    "iconIssues",
    "fontIssues",
    "structureIssues",
    "intentQuestions",
    "variableIssues",
    "imageSlots",
    "contrastIssues",
    "navIssues",
];

/// Marker added when a cap dropped anything.
const TRUNCATED_KEY: &str = "diagnosticsTruncated";

/// Run the detect-only scans on the active page of `state` and return the JSON
/// keys to merge into a write result. The map is empty when the page is clean.
///
/// `state` must already carry the write and any repair passes the caller runs;
/// this function reads it and never mutates it.
pub fn post_write_diagnostics(
    state: &EditorState,
    extras: PostWriteExtras,
    caps: Option<&FeedbackCaps>,
) -> Map<String, Value> {
    let report = collect_design_quality_uncapped(state);
    let mut layout_issues = report.geometry_issues;
    layout_issues.extend(report.layout_issues);
    let mut structure_issues = report.structure_issues;
    structure_issues.extend(extras.structure_notes);
    let mut icon_issues = report.icon_issues;
    let mut font_issues =
        scan_font_issues(state.active_children(), &FontKnowledge::from_state(state));
    let mut intent_questions = report.intent_questions;
    let mut variable_issues = report.variable_issues;
    let mut image_slots = report.image_slots;
    let mut nav_issues = report.nav_issues;
    let mut empty_shells = report.empty_shells;
    let contrast_issues = report.contrast_issues;
    let root_seed_hint = extras.root_seed_hint;

    let mut out = Map::new();
    if layout_issues.is_empty()
        && structure_issues.is_empty()
        && contrast_issues.is_empty()
        && icon_issues.is_empty()
        && font_issues.is_empty()
        && empty_shells.is_empty()
        && intent_questions.is_empty()
        && variable_issues.is_empty()
        && image_slots.is_empty()
        && nav_issues.is_empty()
        && root_seed_hint.is_none()
    {
        return out;
    }

    // Hints are worded from the full lists, before any cap shortens them.
    let hints = hints(
        &layout_issues,
        &icon_issues,
        &intent_questions,
        &variable_issues,
        &image_slots,
        &nav_issues,
        root_seed_hint,
    );
    let contrast_hint_text = (!contrast_issues.is_empty()).then(|| contrast_hint(&contrast_issues));

    let mut truncated = false;
    let mut contrast_value = json!(contrast_issues);
    if let Some(caps) = caps {
        for list in [
            &mut layout_issues,
            &mut icon_issues,
            &mut font_issues,
            &mut structure_issues,
            &mut intent_questions,
            &mut variable_issues,
            &mut image_slots,
            &mut nav_issues,
            &mut empty_shells,
        ] {
            truncated |= cap_lines(list, caps);
        }
        let mut lines: Vec<String> = contrast_issues.iter().map(contrast_line).collect();
        truncated |= cap_lines(&mut lines, caps);
        contrast_value = json!(lines);
    }

    let mut insert_list = |key: &str, items: Vec<String>| {
        if !items.is_empty() {
            out.insert(key.into(), json!(items));
        }
    };
    insert_list("layoutIssues", layout_issues);
    insert_list("iconIssues", icon_issues);
    insert_list("fontIssues", font_issues);
    insert_list("structureIssues", structure_issues);
    insert_list("intentQuestions", intent_questions);
    insert_list("variableIssues", variable_issues);
    insert_list("imageSlots", image_slots);
    if !empty_shells.is_empty() {
        out.insert(
            "shellsRemaining".into(),
            json!(format!(
                "{} - fill each in its own batch; NEVER end the design while any shell is empty (D() a shell you decided against)",
                empty_shells.join(", ")
            )),
        );
    }
    if let Some(hint) = contrast_hint_text {
        out.insert("contrastHint".into(), json!(hint));
        out.insert("contrastIssues".into(), contrast_value);
    }
    if !nav_issues.is_empty() {
        out.insert("navIssues".into(), json!(nav_issues));
    }
    if !hints.is_empty() {
        out.insert("layoutHint".into(), json!(hints.join(" ")));
    }
    if let Some(caps) = caps {
        truncated |= fit_total_bytes(&mut out, caps.max_total_bytes);
        if truncated {
            out.insert(TRUNCATED_KEY.into(), json!(true));
        }
    }
    out
}

/// The detect-only scans with no per-key cap on `structureIssues` (the public
/// `get_design_quality` tool trims that list to 12; a write result is capped
/// later, and only for external callers).
fn collect_design_quality_uncapped(state: &EditorState) -> DesignQualityDiagnostics {
    collect_design_quality_with_structure_cap(state, usize::MAX)
}

fn hints(
    layout_issues: &[String],
    icon_issues: &[String],
    intent_questions: &[String],
    variable_issues: &[String],
    image_slots: &[String],
    nav_issues: &[String],
    root_seed_hint: Option<String>,
) -> Vec<String> {
    let mut hints = Vec::new();
    if !layout_issues.is_empty() {
        hints.push(
            "The resolved layout has the issues above. Fix them with a follow-up batch_design before building the next section."
                .to_string(),
        );
    }
    if !icon_issues.is_empty() {
        hints.push(
            "iconIssues: every icon listed renders as a fallback dot. Fix each with U(id, {\"iconFontName\":\"<glyph>\"}) using a real lucide glyph name (home/search/heart/compass/...)."
                .to_string(),
        );
    }
    if !intent_questions.is_empty() {
        hints.push(
            "intentQuestions are ambiguous: inspect the named nodes and choose explicitly. Do not assume the finalizer will move, resize, delete, or recolor them."
                .to_string(),
        );
    }
    if !variable_issues.is_empty() {
        hints.push(
            "variableIssues name broken references. Replace them deliberately with a token returned by get_variables or a concrete value; nearby colours are not sufficient evidence."
                .to_string(),
        );
    }
    if !image_slots.is_empty() {
        hints.push(
            "imageSlots lists unresolved media slots. Resolve them before continuing: default G requires the exact EMPTY slot id, never its row/card container; if an image is already a direct sibling, use M(imageId, slotId) only when explicit parent/slot hierarchy assigns it there. Do not decide from image subject, aesthetics, or perceived quality."
                .to_string(),
        );
    }
    if !nav_issues.is_empty() {
        hints.push(
            "navIssues: this is a multi-screen app - the listed nav tabs are not wired to switch screens yet. Bind each one's events.onTap exactly as shown; do not guess a different destination."
                .to_string(),
        );
    }
    if let Some(hint) = root_seed_hint {
        hints.push(hint);
    }
    hints
}

/// One contrast finding as a single short line (external callers only).
fn contrast_line(issue: &ContrastIssue) -> String {
    let name = issue
        .node_name
        .as_deref()
        .map(|name| format!(" '{name}'"))
        .unwrap_or_default();
    format!(
        "node {}{name}: contrast {:.1}:1 ({} on {}), needs {}:1",
        issue.node_id, issue.ratio, issue.fg, issue.bg, issue.target
    )
}

/// Keep at most `caps.max_items` lines, each at most `caps.max_item_chars`
/// characters (cut on a char boundary, `...` marks the cut). Returns whether
/// anything was dropped or cut.
pub(super) fn cap_lines(lines: &mut Vec<String>, caps: &FeedbackCaps) -> bool {
    let mut cut = lines.len() > caps.max_items;
    lines.truncate(caps.max_items);
    for line in lines.iter_mut() {
        if line.chars().count() > caps.max_item_chars {
            let keep = caps.max_item_chars.saturating_sub(3);
            let end = line
                .char_indices()
                .nth(keep)
                .map_or(line.len(), |(index, _)| index);
            line.truncate(end);
            line.push_str("...");
            cut = true;
        }
    }
    cut
}

/// Shrink `map` until it serializes within `max_bytes`: hints first, then the
/// shell sentence, then the last item of the longest list, one at a time.
/// Returns whether anything was dropped. Deterministic: ties go to the key
/// that comes first in [`LIST_KEYS`].
fn fit_total_bytes(map: &mut Map<String, Value>, max_bytes: usize) -> bool {
    let size = |map: &Map<String, Value>| Value::Object(map.clone()).to_string().len();
    let mut dropped = false;
    while size(map) > max_bytes {
        dropped = true;
        let hint_key = ["layoutHint", "contrastHint", "shellsRemaining"]
            .into_iter()
            .find(|key| map.contains_key(*key));
        if let Some(key) = hint_key {
            map.remove(key);
            continue;
        }
        let longest = LIST_KEYS
            .iter()
            .filter_map(|key| {
                let len = map.get(*key)?.as_array()?.len();
                (len > 0).then_some((len, *key))
            })
            .fold(None::<(usize, &str)>, |best, candidate| match best {
                Some((len, _)) if len >= candidate.0 => best,
                _ => Some(candidate),
            });
        let Some((_, key)) = longest else {
            break;
        };
        let emptied = map
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .is_some_and(|items| {
                items.pop();
                items.is_empty()
            });
        if emptied {
            map.remove(key);
        }
    }
    dropped
}
