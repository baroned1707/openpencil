//! The in-app design loop's system prompt: protocol + skills, then the
//! per-run context.
//!
//! Order matters for Anthropic prompt caching, which keys on the exact prefix:
//! the STATIC part (protocol and prompt-matched skills) comes first, the
//! per-run part (the user's design decisions, the canvas brief) after it.
//!
//! The user's own decisions — the document's design.md, the style guide pinned
//! in the Asset Center, and the current selection — used to reach only the
//! orchestrator pipeline. They ride the loop through ONE bounded block built
//! here from the orchestrator's own serializers
//! (`op_orchestrator::design_md_prompt_text`, `pinned_style_guide_prompt`). A style
//! the user picked from the composer's suggestions (`chat.chosen_style`) joins
//! them as one short paragraph for that run only.

use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::EditorState;

/// Upper bound for the design.md text in the block.
pub const DESIGN_MD_MAX_BYTES: usize = 4 * 1024;
/// Upper bound for the pinned style-guide block.
pub const PINNED_STYLE_MAX_BYTES: usize = 6 * 1024;
/// Selected nodes listed individually; the rest are counted.
pub const SELECTION_MAX_NODES: usize = 20;

const DESIGN_MD_TRUNCATED: &str = "[design.md truncated]";

/// Cut `text` to at most `max_bytes` on a char boundary and mark the cut.
fn truncate_with_marker(text: &str, max_bytes: usize, marker: &str) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n{marker}", text[..end].trim_end())
}

/// One selected node as `- id "name" (type, WxH)`.
fn selection_line(state: &EditorState, id: &op_editor_core::NodeId) -> Option<String> {
    let node = op_editor_core::walkers::find_node(state.active_children(), id)?;
    let kind = serde_json::to_value(node)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|t| t.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| "node".to_string());
    let name = node.base().name.as_deref().unwrap_or("(unnamed)");
    let size = match (node.width_px(), node.height_px()) {
        (Some(w), Some(h)) => format!("{}x{}", w as i64, h as i64),
        _ => "auto".to_string(),
    };
    Some(format!("- {} \"{name}\" ({kind}, {size})", id.as_str()))
}

/// The user's design decisions for this run, or `None` when the document has
/// no design.md, no live pin and no selection (the prompt then stays
/// byte-identical to the one without this block).
///
/// Precedence follows the orchestrator: design.md outranks a pinned guide, so
/// the pin is left out when a design.md exists. `model` is the selected
/// model's capability id; it only picks the pin block's tier.
pub fn design_user_context_block(state: &EditorState, model: Option<&str>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    let design_md = state
        .doc
        .design_md
        .as_ref()
        .map(op_orchestrator::design_md_prompt_text)
        .filter(|text| !text.is_empty());
    if let Some(text) = &design_md {
        parts.push(format!(
            "### design.md (the project's design system)\n{}",
            truncate_with_marker(text, DESIGN_MD_MAX_BYTES, DESIGN_MD_TRUNCATED)
        ));
    } else if let Some(pin) = op_orchestrator::pinned_style_guide_prompt(
        state.editor_ui.pinned_style_guide.as_deref(),
        model,
        PINNED_STYLE_MAX_BYTES,
    ) {
        parts.push(format!(
            "### Pinned style guide: {}\n{}",
            pin.id, pin.instruction
        ));
    }

    let selected: Vec<&op_editor_core::NodeId> = state
        .selection
        .set
        .iter()
        .filter(|id| id.is_real())
        .collect();
    if !selected.is_empty() {
        let mut lines: Vec<String> = selected
            .iter()
            .take(SELECTION_MAX_NODES)
            .filter_map(|id| selection_line(state, id))
            .collect();
        if selected.len() > SELECTION_MAX_NODES {
            lines.push(format!(
                "- ...and {} more selected",
                selected.len() - SELECTION_MAX_NODES
            ));
        }
        if !lines.is_empty() {
            parts.push(format!(
                "### Current selection ({} node(s) — the user is pointing at these; \"this\" / \"it\" means them)\n{}",
                selected.len(),
                lines.join("\n")
            ));
        }
    }

    // The style the user picked from the composer's suggestions for this run
    // only. Last, so design.md and a pinned guide above it read as the
    // stronger decisions (the paragraph also says so itself).
    if let Some(paragraph) = state.chat.chosen_style.as_ref().and_then(|chosen| {
        op_ai_skills::design_styles_choice::chosen_style_paragraph(&chosen.style_ref)
    }) {
        parts.push(format!("### Style chosen for this run\n{paragraph}"));
    }

    if parts.is_empty() {
        return None;
    }
    Some(format!(
        "## The user's design decisions for this task\n\n\
         These come from the user's own project, not from you or from a default. They are \
         decisions: follow them over your own defaults and over any style guide you would \
         otherwise pick.\n\n{}",
        parts.join("\n\n")
    ))
}

/// The loop's complete system prompt for one run: protocol + prompt-matched
/// skills (static), then the user's design decisions, then the multi-screen
/// canvas brief. The image self-check scope stays the last section.
pub fn design_loop_system_prompt(
    state: &EditorState,
    user_text: &str,
    model: Option<&str>,
) -> String {
    let mut system_prompt = op_ai_skills::design_agent_system_prompt_with_skills(user_text);
    if let Some(context) = design_user_context_block(state, model) {
        system_prompt.push_str("\n\n---\n\n");
        system_prompt.push_str(&context);
        op_ai_skills::append_image_self_check_scope(&mut system_prompt);
    }
    if let Some(brief) = crate::design_context::design_context_brief(state) {
        system_prompt.push_str("\n\n---\n\n");
        system_prompt.push_str(&brief);
        op_ai_skills::append_image_self_check_scope(&mut system_prompt);
    }
    system_prompt
}

#[cfg(test)]
#[path = "design_loop_prompt_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "design_loop_prompt_choice_tests.rs"]
mod choice_tests;
