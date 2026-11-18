//! Per-tool hints in the `tools/list` catalog: MCP `annotations` plus the
//! Claude Code `_meta` keys.
//!
//! The hints are DERIVED from the one classification table,
//! `tool_profile::TOOL_PROFILES`, so a new tool is covered the moment it is
//! classified there (a parity test already fails the build until it is):
//!
//! - `readOnlyHint` — the table's `Read` access on the in-memory surface,
//!   minus the tools below that the table calls `Read` only because they leave
//!   the DOCUMENT alone while they still write a file, a plan store or a
//!   process. A `Read` tool on any other surface must be reviewed into
//!   [`READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT`] first.
//! - `openWorldHint` — true exactly for the `OutboundNetwork` surface.
//! - `destructiveHint` / `idempotentHint` — a write's own facts, listed by
//!   name in [`DESTRUCTIVE_WRITES`] and [`IDEMPOTENT_WRITES`] (plus the
//!   `set_*` rule). Both lists are checked against the table so a rename or
//!   removal cannot leave a stale entry.
//!
//! MCP's defaults are `readOnlyHint=false`, `destructiveHint=true`,
//! `idempotentHint=false`, `openWorldHint=true`
//! (`@modelcontextprotocol/sdk` `ToolAnnotationsSchema`). A client that sees
//! no hint assumes the worst, so every tool states `openWorldHint`; a
//! read-only tool states `readOnlyHint:true`; a write states `destructiveHint`
//! (and `idempotentHint:true` when true). `readOnlyHint:false` and
//! `idempotentHint:false` are left out because they are the defaults, and the
//! catalog is paid for in agent context on every turn.
//!
//! `_meta` keys are Claude Code's (https://code.claude.com/docs/en/mcp):
//! `anthropic/alwaysLoad` and `anthropic/maxResultSizeChars`.
//!
//! The hints are added here, to the finished `tools/list` entries, and never
//! to the `TOOL_SCHEMAS` constants: those also become the in-process design
//! agent's LLM tool definitions, where unknown keys do not belong.

use super::tool_catalog::LEAN_TOOLS;
use super::tool_profile::{profile_for, schema_name, ToolAccess, ToolSurface};

/// Tools the access table calls `Read` (they do not change the document)
/// that still change something else, so they are not read-only.
const READ_BUT_WRITES_ELSEWHERE: &[&str] = &[
    // With a `filePath` that does not exist, creates an empty .op file there.
    "open_document",
    // Write a file the caller names.
    "save_document",
    "save_theme_preset",
    "export_frames",
    "export_deck",
    // The process-wide codegen plan store: each call adds, replaces or
    // consumes a plan.
    "codegen_plan",
    "codegen_submit_chunk",
    "codegen_assemble",
    "codegen_clean",
    // Not read-only by contract: in the in-app design agent it starts
    // sub-agent runs (an MCP connection gets a refusal instead).
    "spawn_agents",
];

/// Tools the access table calls `Read` on a surface beyond the in-memory
/// document that were reviewed and only READ it (directory listings, the
/// shipped style guides, the host's own logs and diagnostics). A `Read` tool
/// on any other surface is not advertised read-only until it is added here,
/// so a new file-writing "read" cannot be mislabelled by omission.
const READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT: &[&str] = &[
    "debug_logs_tail",
    "debug_screenshot",
    "debug_validation_report",
    "list_style_guides",
    "list_theme_presets",
];

/// Writes that delete, replace or overwrite existing content wholesale, or
/// step the document back and forth through its history.
const DESTRUCTIVE_WRITES: &[&str] = &[
    // Programs and operations that may delete (D) or replace (R) nodes, and
    // passes that dedupe, reshape or regenerate existing ones.
    "apply_design_system",
    "batch_design",
    "design_content",
    "design_refine",
    "design_skeleton",
    "finalize_design",
    "run_design_agent",
    "update_generator_params",
    "cut_selected",
    "delete_component",
    "delete_node",
    "delete_page",
    "delete_selected",
    "delete_variable",
    "detach_generator",
    "redo",
    "remove_node_effect",
    "remove_page",
    "replace_all_matching_properties",
    "replace_node",
    "set_design_md",
    "set_themes",
    "set_variables",
    "undo",
];

/// Writes (beyond every `set_*` tool) whose repetition with the same
/// arguments leaves the document as the first call did.
const IDEMPOTENT_WRITES: &[&str] = &[
    "align_selected",
    "apply_design_system",
    "clear_selection",
    "delete_component",
    "delete_node",
    "delete_page",
    "delete_selected",
    "delete_variable",
    "detach_generator",
    "load_theme_preset",
    "move_node",
    "remove_page",
    "rename_component",
    "rename_page",
    "rename_variable",
    "update_node",
    "upsert_component",
    "upsert_screen",
    "upsert_variables",
];

/// Tools the table calls `Read` that write a file and whose repetition with
/// the same arguments leaves the same file (`save_document`: a copy to the
/// same `filePath`, or the document saved to its own file, twice). Beyond
/// [`READ_BUT_WRITES_ELSEWHERE`] they are advertised idempotent; they are not
/// in [`IDEMPOTENT_WRITES`] because that list is checked against `Write` rows.
const IDEMPOTENT_FILE_WRITES: &[&str] = &["save_document"];

/// Tools whose result can be large on purpose (whole subtrees, generated
/// source, the design.md), so Claude Code should not persist it to a file
/// at its 50,000-character default. The documented ceiling is 500,000.
const LARGE_RESULT_TOOLS: &[&str] = &[
    "batch_get",
    "codegen_assemble",
    "codegen_export",
    "get_design_md",
    "read_nodes",
    "snapshot_layout",
];

/// Claude Code's per-tool result cap for [`LARGE_RESULT_TOOLS`].
pub(super) const LARGE_RESULT_CHARS: usize = 200_000;

/// What `tools/list` states about one tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ToolHints {
    pub read_only: bool,
    /// Meaningful only for a write.
    pub destructive: bool,
    /// Meaningful only for a write.
    pub idempotent: bool,
    pub open_world: bool,
}

/// The hints for `name`. A name outside the table is a per-document kit
/// `insert_<component>` tool: an additive in-memory write.
pub(super) fn hints_for(name: &str) -> ToolHints {
    let Some(profile) = profile_for(name) else {
        return ToolHints {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        };
    };
    ToolHints {
        read_only: profile.access == ToolAccess::Read
            && !READ_BUT_WRITES_ELSEWHERE.contains(&name)
            && (profile.surface == ToolSurface::InMemory
                || READS_THAT_ONLY_READ_OUTSIDE_THE_DOCUMENT.contains(&name)),
        destructive: DESTRUCTIVE_WRITES.contains(&name),
        idempotent: name.starts_with("set_")
            || IDEMPOTENT_WRITES.contains(&name)
            || IDEMPOTENT_FILE_WRITES.contains(&name),
        open_world: profile.surface == ToolSurface::OutboundNetwork,
    }
}

/// The `annotations` object text for `name`.
fn annotations_json(name: &str) -> String {
    let hints = hints_for(name);
    if hints.read_only {
        return format!(
            r#"{{"readOnlyHint":true,"openWorldHint":{}}}"#,
            hints.open_world
        );
    }
    let idempotent = if hints.idempotent {
        r#""idempotentHint":true,"#
    } else {
        ""
    };
    format!(
        r#"{{"destructiveHint":{},{idempotent}"openWorldHint":{}}}"#,
        hints.destructive, hints.open_world
    )
}

/// The `_meta` object text for `name`, if it has any key.
fn meta_json(name: &str) -> Option<String> {
    let mut keys: Vec<String> = Vec::new();
    if LEAN_TOOLS.contains(&name) {
        keys.push(r#""anthropic/alwaysLoad":true"#.to_string());
    }
    if LARGE_RESULT_TOOLS.contains(&name) {
        keys.push(format!(
            r#""anthropic/maxResultSizeChars":{LARGE_RESULT_CHARS}"#
        ));
    }
    (!keys.is_empty()).then(|| format!("{{{}}}", keys.join(",")))
}

/// Append `annotations` (and `_meta`, when the tool has any) to one
/// `tools/list` entry. An entry that is not a `{...}` object, or has no name,
/// is returned unchanged.
pub(super) fn decorate_entry(entry: &str) -> String {
    let (Some(name), Some(body)) = (schema_name(entry), entry.trim_end().strip_suffix('}')) else {
        return entry.to_string();
    };
    let mut out = format!(r#"{body},"annotations":{}"#, annotations_json(&name));
    if let Some(meta) = meta_json(&name) {
        out.push_str(r#","_meta":"#);
        out.push_str(&meta);
    }
    out.push('}');
    out
}

#[cfg(test)]
#[path = "tool_annotations_tests.rs"]
mod tests;
