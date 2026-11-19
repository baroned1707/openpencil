//! Repairs every bare-MCP document write runs after its command applied and
//! before the document is saved (headless, live editor, web daemon), shared
//! through [`run_write_repairs_after_apply`].

use op_chat_agent::design_agent_tools::{rename_unknown_icons, IconNameSnapshot};
use op_editor_core::EditorState;

pub(crate) fn normalize_mobile_screens_after_apply(
    state: &mut EditorState,
) -> op_chat_agent::MobileNormalizeReport {
    let report = op_chat_agent::normalize_mobile_screens(state);
    let mut changes = Vec::new();
    if report.status_bars_inserted > 0 {
        changes.push(format!(
            "status_bars_inserted={}",
            report.status_bars_inserted
        ));
    }
    if report.status_bars_replaced > 0 {
        changes.push(format!(
            "status_bars_replaced={}",
            report.status_bars_replaced
        ));
    }
    if report.duplicate_status_bars_removed > 0 {
        changes.push(format!(
            "duplicate_status_bars_removed={}",
            report.duplicate_status_bars_removed
        ));
    }
    if report.viewport_heights_fixed > 0 {
        changes.push(format!(
            "viewport_heights_fixed={}",
            report.viewport_heights_fixed
        ));
    }
    if !changes.is_empty() {
        eprintln!(
            "openpencil-desktop mcp: mobile normalization: {}",
            changes.join(", ")
        );
    }
    report
}

pub(crate) fn normalize_icon_paths_after_apply(
    state: &mut EditorState,
) -> op_editor_core::icon_path_normalize::IconPathNormalizeReport {
    let report = op_editor_core::icon_path_normalize::normalize_icon_paths(
        state,
        op_editor_ui::widgets::icons::lucide_name_for_path_d,
    );
    if report != Default::default() {
        eprintln!(
            "openpencil-desktop mcp: icon path normalization: converted_to_icon_font={}, refit_uniform={}",
            report.converted_to_icon_font, report.refit_uniform
        );
    }
    report
}

/// Run the cheap, idempotent repair passes shared by every bare-MCP document
/// write after the command itself has applied and before it saves.
/// `icon_names_before` is [`IconNameSnapshot::capture`] taken BEFORE the
/// command: the glyph-name auto-rename only touches icons the write added or
/// changed.
pub(crate) fn run_write_repairs_after_apply(
    state: &mut EditorState,
    icon_names_before: &IconNameSnapshot,
) -> bool {
    normalize_mobile_screens_after_apply(state);
    normalize_icon_paths_after_apply(state);
    rename_unknown_icons(state, icon_names_before);
    let mobile_nav_reflowed = op_orchestrator::repair_mobile_trailing_nav_reflow(state);
    let image_slots_materialized =
        op_orchestrator::cleanup_image_slots::materialize_empty_image_fill_slots(state);
    mobile_nav_reflowed || image_slots_materialized
}
