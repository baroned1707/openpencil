//! MCP tool `finalize_design` — run OpenPencil's post-generation repair
//! passes over the document from a plain MCP call.
//!
//! External models that drive the editor through the bare MCP `batch_design`
//! surface never get the deterministic cleanup/repair backstop the built-in
//! design agent applies after a generation. This tool closes that gap: it runs
//! the exact App whole-document finalizer (`op_orchestrator::
//! record_loop_finalize_counted`) and reports the `RepairSummary` that path
//! surfaces as its quality credential.
//!
//! ## How the mutation reaches the host
//!
//! MCP tools are snapshots: they cannot mutate the live `EditorState`, they
//! return `EditorCommand`s the host applies (and, in file-backed mode, saves).
//! The orchestrator records sink-driven repairs and converts direct semantic
//! passes to same-id shallow patches in App order. It atomically replays and
//! compares the canonical document before this tool returns the proven command
//! sequence as ONE `EditorCommand::Batch`.
//!
//! ## Idempotence
//!
//! The passes are idempotent; running the tool twice over an already-finalized
//! document lands (near) zero repairs on the second call.
//!
//! ## Echo-only advisories
//!
//! - `section-structure-drift`: sibling nodes have inconsistent structure.
//! - `board-trailing-void`: a card/deck board has significant empty space.
//! - `board-format-drift`: a card board's aspect ratio drifted from 3:4/1:1.
//! - `shader-invalid`: a shader fill is invalid and will degrade to flat colour.
//! - `shader-budget`: a shader fill is expensive (exceeds GPU budget for the design form).

use std::collections::{BTreeMap, BTreeSet};

use jian_ops_schema::node::PenNode;
use op_chat_agent::design_agent_tools::{post_write_diagnostics, FeedbackCaps, PostWriteExtras};
use op_design_lint::design_form::classify_root_form_node;
use op_design_lint::detectors::detect_shader_budget;
use op_editor_core::{EditorCommand, EditorState, PenNodeExt};
use op_mcp::{McpTool, ToolErrorCode, ToolOutcome};
use op_orchestrator::repair_summary::RepairSummary;

/// The `finalize_design` tool: a snapshot of the document at registration
/// time plus nothing else — the cleanup runs against a clone inside `call`.
pub struct FinalizeDesignTool {
    state: EditorState,
}

/// Snapshot constructor, same shape as the other read/write tool snapshots.
pub fn finalize_design_snapshot(doc: &EditorState) -> FinalizeDesignTool {
    FinalizeDesignTool { state: doc.clone() }
}

impl McpTool for FinalizeDesignTool {
    fn name(&self) -> &str {
        "finalize_design"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let root_ids: Vec<String> = match parse_root_ids(args, &self.state) {
            Ok(ids) => ids,
            Err(message) => return ToolOutcome::Err(ToolErrorCode::InvalidArgument, message),
        };
        let recorded = match op_orchestrator::record_loop_finalize_counted(&self.state) {
            Ok(recorded) => recorded,
            Err(error) => {
                return ToolOutcome::Err(
                    ToolErrorCode::Internal,
                    format!("finalize_design could not prove command replay parity: {error}"),
                )
            }
        };
        let op_orchestrator::RecordedLoopFinalize {
            state,
            summary,
            commands,
        } = recorded;
        // Post-cleanup echo-only structural advisories (DS P2-a item ③):
        // every parent node of the FINAL document runs the same
        // sibling-structure-drift detector the pre-insertion self-check
        // uses, and hits are reported — never repaired, never counted in
        // the repair tally, never written back as commands. The
        // model-in-the-loop fixes them via batch_design / update_node and
        // calls finalize_design again (see the schema description).
        let advisories = op_orchestrator::orchestration_self_check::collect_section_structure_drift(
            state.active_children(),
        );
        // DS P2-b item C: board-trailing-void findings ride the SAME
        // echo-only advisory channel — reported, never repaired, never
        // counted in the repair tally. A fixed Card/Deck board whose void is
        // still >= 25% after the cleanup passes (incl. the centre repair)
        // holds too little content for any repair to rescue; the advisory
        // tells the caller to add density.
        let void_advisories =
            op_orchestrator::board_trailing_void::collect_board_trailing_void(&state);
        // DS P2-d item ②: card format drift rides the same channel too. A
        // Card board whose authored aspect passed the 3:4 / 1:1 regular band
        // (e.g. the text-wrap reflow growing 1440 → 2116) is an
        // informational finding — whether long-form output is acceptable is
        // a product decision, so the advisory names the ratio and both
        // directions and repairs nothing.
        let format_drift = op_orchestrator::board_trailing_void::collect_board_format_drift(&state);
        // Shader-budget findings: GPU cost of shader fills (blocking if the
        // shader is invalid and degrades to a flat colour; informational if
        // just expensive). Each root form is classified once to determine the
        // budget tier, then all issues are partitioned by severity.
        let mut shader_blocking = Vec::new();
        let mut shader_informational = Vec::new();
        for root in state.active_children() {
            let form = classify_root_form_node(root);
            let issues = detect_shader_budget(root, form);
            for issue in issues {
                if issue.severity == op_design_lint::IssueSeverity::Warning {
                    shader_blocking.push(issue);
                } else {
                    shader_informational.push(issue);
                }
            }
        }
        let json = finalize_result_json(
            &summary,
            root_ids.len(),
            &advisories,
            &void_advisories,
            &format_drift,
            RenderFindings {
                shader_blocking: &shader_blocking,
                shader_informational: &shader_informational,
                clipped_content:
                    &op_orchestrator::geometry_validation::fixed_board_content_diagnostics(&state),
                changed_board_sizes: &fixed_board_size_changes(&self.state, &state),
                remaining_issues: &remaining_write_time_issues(&state),
            },
        );
        if commands.is_empty() {
            ToolOutcome::OkJson(json)
        } else {
            ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands })
        }
    }
}

/// `root_ids`: optional JSON array string, comma-separated string, or omitted
/// for the required whole-document scope (every top-level root on the active
/// page). Blank input is treated as omitted. An explicit subset is rejected:
/// the App-equivalent finalizer is intentionally not a per-root transform.
fn parse_root_ids(
    args: &BTreeMap<String, String>,
    state: &EditorState,
) -> Result<Vec<String>, String> {
    let raw = args
        .get("root_ids")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty());
    let Some(raw) = raw else {
        return Ok(default_root_ids(state));
    };
    let ids: Vec<String> = if raw.starts_with('[') {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::Array(items)) => items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
            Ok(_) => {
                return Err("root_ids must be a JSON array of node id strings".to_string());
            }
            Err(_) => {
                return Err("root_ids must be a JSON array of node id strings".to_string());
            }
        }
    } else {
        raw.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };
    if ids.is_empty() {
        return Err("root_ids must name at least one node".to_string());
    }
    let requested: BTreeSet<String> = ids.into_iter().collect();
    let all_roots = default_root_ids(state);
    let all_root_set: BTreeSet<String> = all_roots.iter().cloned().collect();
    if requested.len() != all_roots.len() || requested != all_root_set {
        return Err(
            "root_ids must include every top-level root on the active page; the whole-document finalizer requires every active root"
                .to_string(),
        );
    }
    // Preserve active-page document order for reporting and deterministic
    // diagnostics. Duplicate explicit ids have already been harmlessly
    // collapsed by the set equality check above.
    Ok(all_roots)
}

/// Every top-level root on the active page — the tool's required scope.
fn default_root_ids(state: &EditorState) -> Vec<String> {
    state
        .active_children()
        .iter()
        .map(|node| node.id_str().to_string())
        .collect()
}

/// The human-readable repair summary, rendered through the same
/// `quality_credential` surface the built-in agent loop shows its users, plus
/// the structured per-category tally so an MCP client can reason about it.
///
/// `advisories` (DS P2-a item ③) are the echo-only structure-drift findings,
/// `void_advisories` (DS P2-b item C) the board-trailing-void ones,
/// `format_drift` (DS P2-d item ②) the card format-drift ones, and
/// `shader_blocking` / `shader_informational` are shader fill diagnostics:
/// the blocking ones (shader-invalid) are added to advisories and keep
/// `complete=false`; the informational ones (shader-budget) ride a separate
/// echo-only `informational` array that never affects `complete`. None are
/// edits and therefore never inflate the repair tally.
struct RenderFindings<'a> {
    shader_blocking: &'a [op_design_lint::Issue],
    shader_informational: &'a [op_design_lint::Issue],
    clipped_content: &'a [String],
    changed_board_sizes: &'a [String],
    /// Write-time issues still open on the finalized roots (see
    /// [`remaining_write_time_issues`]); empty when none.
    remaining_issues: &'a serde_json::Map<String, serde_json::Value>,
}

/// The write-time scan keys that make a finalize verdict `warn`.
///
/// `imageSlots` is deliberately not one of them: finalize itself creates the
/// empty slots that `enrich_images` fills afterwards, so counting them would
/// make every run `warn`. Structure / shell / nav keys are not part of the
/// write-time quality contract this verdict reports.
const WARN_KEYS: [&str; 5] = [
    "layoutIssues",
    "iconIssues",
    "contrastIssues",
    "fontIssues",
    "contrastUncertain",
];

/// At most this many lines per key ride along in `remainingIssues`; the full
/// count is in `next`.
const REMAINING_LINES_PER_KEY: usize = 5;

/// Run the SAME scans a design write runs (`post_write_diagnostics`, the one
/// function behind `batch_design`'s feedback) over the finalized document and
/// keep the [`WARN_KEYS`] lists, as short lines. The finalizer has already
/// applied its repairs to `state`, so no write repairs run here.
fn remaining_write_time_issues(state: &EditorState) -> serde_json::Map<String, serde_json::Value> {
    // No item cap here so the counts are exact; the lines are trimmed when
    // they are put in the result.
    let counting_caps = FeedbackCaps {
        max_items: usize::MAX,
        max_item_chars: 240,
        max_total_bytes: usize::MAX,
        contrast_as_lines: true,
    };
    let mut all = post_write_diagnostics(state, PostWriteExtras::default(), Some(&counting_caps));
    WARN_KEYS
        .iter()
        .filter_map(|key| {
            let value = all.remove(*key)?;
            value
                .as_array()
                .is_some_and(|items| !items.is_empty())
                .then(|| (key.to_string(), value))
        })
        .collect()
}

/// The verdict a finalize result gives and the one line telling the caller
/// what to do next.
///
/// Rules:
/// - `fail`: `blockingAdvisoryCount > 0`, i.e. `complete == false`. The design
///   must not be presented as finished.
/// - `warn`: `complete` is true but the write-time scans still find a layout,
///   icon, contrast, font or contrast-uncertain item on the finalized roots.
/// - `pass`: neither.
///
/// An uncertain-only `warn` cannot be "fixed" away (a scrim over a photo still
/// reads as translucent), so its `next` tells the caller to verify with a
/// screenshot and finish, instead of looping on finalize.
fn verdict_and_next(
    complete: bool,
    blocking_advisory_count: usize,
    remaining: &serde_json::Map<String, serde_json::Value>,
) -> (&'static str, String) {
    if !complete || blocking_advisory_count > 0 {
        return (
            "fail",
            format!(
                "{blocking_advisory_count} blocking advisory(ies): fix each with batch_design or update_node, then call finalize_design again."
            ),
        );
    }
    if remaining.is_empty() {
        return (
            "pass",
            "No open issues found; present the design (run enrich_images first if image slots are still empty)."
                .to_string(),
        );
    }
    let counts = |skip_uncertain: bool| {
        WARN_KEYS
            .iter()
            .filter(|key| !(skip_uncertain && **key == "contrastUncertain"))
            .filter_map(|key| {
                let count = remaining.get(*key)?.as_array()?.len();
                Some(format!("{key} {count}"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let only_uncertain = remaining.keys().all(|key| key == "contrastUncertain");
    let next = if only_uncertain {
        format!(
            "Text sits on an image, gradient or translucent fill ({}): check it with get_screenshot and finish if it reads well; add a scrim or solid backing if not.",
            counts(false)
        )
    } else {
        format!(
            "Open issues ({}), listed under remainingIssues: fix them with batch_design or update_node, then call finalize_design again.",
            counts(false)
        )
    };
    ("warn", next)
}

/// `remaining` with each list cut to [`REMAINING_LINES_PER_KEY`] lines.
fn remaining_issues_for_result(
    remaining: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    remaining
        .iter()
        .map(|(key, value)| {
            let lines: Vec<serde_json::Value> = value
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .take(REMAINING_LINES_PER_KEY)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            (key.clone(), serde_json::Value::Array(lines))
        })
        .collect()
}

fn fixed_board_size_changes(before: &EditorState, after: &EditorState) -> Vec<String> {
    before.active_children().iter().filter_map(|node| {
        let PenNode::Frame(frame) = node else { return None; };
        if frame.container.clip_content == Some(false) { return None; }
        let (width,height) = (node.width_px()?,node.height_px()?);
        if width <= 0.0 || height <= 0.0 { return None; }
        let current = after.active_children().iter().find(|n| n.id_str() == node.id_str())?;
        let (new_width,new_height) = (current.width_px()?,current.height_px()?);
        ((width-new_width).abs()>0.5 || (height-new_height).abs()>0.5).then(|| format!(
            "fixed-board-size-changed: {} changed from {width:.0}×{height:.0} to {new_width:.0}×{new_height:.0} during finalization. Restore the requested viewport and reflow its content before declaring the delivery complete.",node.id_str()
        ))
    }).collect()
}

fn finalize_result_json(
    summary: &RepairSummary,
    roots: usize,
    advisories: &[op_orchestrator::orchestration_self_check::SectionStructureDriftAdvisory],
    void_advisories: &[op_orchestrator::board_trailing_void::BoardTrailingVoidAdvisory],
    format_drift: &[op_orchestrator::board_trailing_void::BoardFormatDriftAdvisory],
    findings: RenderFindings<'_>,
) -> String {
    let quality = crate::quality_credential::quality_summary_from_repairs(summary);
    let credential =
        crate::quality_credential::quality_credential_line_with_records(&quality, None)
            .unwrap_or_else(|| "nothing checked".to_string());
    let categories: Vec<serde_json::Value> = op_orchestrator::repair_summary::CheckCategory::ALL
        .iter()
        .filter_map(|category| {
            let checked = summary.checked().contains(category);
            if !checked {
                return None;
            }
            Some(serde_json::json!({
                "category": category.key(),
                "checked": true,
                "repairs": summary.repairs_for(*category),
            }))
        })
        .collect();
    let records: Vec<serde_json::Value> = summary
        .records()
        .iter()
        .map(|record| {
            serde_json::json!({
                "pass": record.pass,
                "category": record.category.key(),
                "nodeId": record.node_id,
                "nodeName": record.node_name,
                "detail": record.detail,
            })
        })
        .collect();
    let render = |code: &str, node_ids: &[String], message: &str| {
        serde_json::json!({
            "code": code,
            "nodeIds": node_ids,
            "message": message,
        })
    };
    let mut advisories_json: Vec<serde_json::Value> = advisories
        .iter()
        .map(|advisory| render(advisory.code, &advisory.node_ids, &advisory.message))
        .collect();
    advisories_json.extend(
        void_advisories
            .iter()
            .map(|advisory| render(advisory.code, &advisory.node_ids, &advisory.message)),
    );
    advisories_json.extend(
        findings
            .changed_board_sizes
            .iter()
            .map(|message| render("fixed-board-size-changed", &[], message)),
    );
    advisories_json.extend(
        format_drift
            .iter()
            .map(|advisory| render(advisory.code, &advisory.node_ids, &advisory.message)),
    );
    // Shader-invalid (blocking) advisories are appended BEFORE complete/count are
    // computed so they gate completion like other blocking advisories.
    advisories_json.extend(findings.shader_blocking.iter().map(|issue| {
        render(
            "shader-invalid",
            std::slice::from_ref(&issue.node_id),
            &issue.reason,
        )
    }));
    advisories_json.extend(
        findings
            .clipped_content
            .iter()
            .map(|message| render("fixed-board-content-outside", &[], message)),
    );
    let complete = advisories_json.is_empty();
    let blocking_advisory_count = advisories_json.len();
    // Shader-budget (informational) advisories ride a separate always-present
    // array that never affects complete or blockingAdvisoryCount.
    let informational_json: Vec<serde_json::Value> = findings
        .shader_informational
        .iter()
        .map(|issue| {
            render(
                "shader-budget",
                std::slice::from_ref(&issue.node_id),
                &issue.reason,
            )
        })
        .collect();
    let (verdict, next) =
        verdict_and_next(complete, blocking_advisory_count, findings.remaining_issues);
    let mut result = serde_json::json!({
        "roots": roots,
        "complete": complete,
        "verdict": verdict,
        "next": next,
        "blockingAdvisoryCount": blocking_advisory_count,
        "checkedCategories": categories,
        "repairs": summary.total_repairs(),
        "repairRecords": records,
        "advisories": advisories_json,
        "informational": informational_json,
        "notes": summary.notes(),
        "summary": credential.trim().to_string(),
    });
    if !findings.remaining_issues.is_empty() {
        result["remainingIssues"] =
            serde_json::Value::Object(remaining_issues_for_result(findings.remaining_issues));
    }
    result.to_string()
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    #[test]
    fn hidden_content_blocks_completion_even_when_export_dimensions_are_correct() {
        let issues = vec!["fixed-board-content-outside: footer is outside the artboard".into()];
        let result = finalize_result_json(
            &RepairSummary::default(),
            1,
            &[],
            &[],
            &[],
            RenderFindings {
                shader_blocking: &[],
                shader_informational: &[],
                clipped_content: &issues,
                changed_board_sizes: &[],
                remaining_issues: &Default::default(),
            },
        );
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["complete"], false);
        assert_eq!(result["blockingAdvisoryCount"], 1);
        assert_eq!(
            result["advisories"][0]["code"],
            "fixed-board-content-outside"
        );
        assert_eq!(result["repairs"], 0);
        assert_eq!(result["verdict"], "fail");
        assert!(result["next"].as_str().unwrap().contains("1 blocking"));
    }

    /// Invariant: a blocking advisory is `fail` even when write-time issues are
    /// also open (the blocking one is what to fix first); `warn` needs
    /// `complete`; nothing open is `pass`.
    #[test]
    fn verdict_rules_fail_before_warn_before_pass() {
        let open: serde_json::Map<String, serde_json::Value> =
            serde_json::from_value(serde_json::json!({"layoutIssues": ["a", "b"]})).unwrap();
        let none = serde_json::Map::new();
        assert_eq!(verdict_and_next(false, 1, &open).0, "fail");
        assert_eq!(verdict_and_next(false, 0, &none).0, "fail");
        let (verdict, next) = verdict_and_next(true, 0, &open);
        assert_eq!(verdict, "warn");
        assert!(next.contains("layoutIssues 2"), "{next}");
        assert_eq!(verdict_and_next(true, 0, &none).0, "pass");
    }

    #[test]
    fn a_finalizer_cannot_claim_success_by_growing_the_original_fixed_viewport() {
        let make = |height, clip| {
            EditorState::from_document(serde_json::from_value(serde_json::json!({
            "version":"1.0.0","children":[{"type":"frame","id":"board","width":390,"height":height,"clipContent":clip}]
        })).unwrap())
        };
        let before = make(844, true);
        let after = make(898, true);
        let issues = fixed_board_size_changes(&before, &after);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("844") && issues[0].contains("898"));
        let result = finalize_result_json(
            &RepairSummary::default(),
            1,
            &[],
            &[],
            &[],
            RenderFindings {
                shader_blocking: &[],
                shader_informational: &[],
                clipped_content: &[],
                changed_board_sizes: &issues,
                remaining_issues: &Default::default(),
            },
        );
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result["complete"], false);
        assert!(fixed_board_size_changes(&make(844, false), &make(898, false)).is_empty());
    }
}
