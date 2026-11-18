//! The style a user picked from the composer's suggestions, in the loop prompt.

use crate::design_loop_prompt::{design_loop_system_prompt, design_user_context_block};
use jian_ops_schema::DesignMdSpec;
use op_ai_skills::design_styles_choice::{CHOSEN_STYLE_MARKER, MAX_CHOSEN_STYLE_BYTES};
use op_editor_core::{EditorState, StyleSuggestion};

const PROMPT: &str = "design a fintech dashboard for a small bank";

fn chosen(style_ref: &str) -> StyleSuggestion {
    let record = op_ai_skills::style_by_ref(style_ref).expect("embedded style");
    StyleSuggestion {
        style_ref: record.reference.clone(),
        name: record.name.clone(),
        blurb: String::new(),
    }
}

fn with_choice(style_ref: &str) -> EditorState {
    let mut state = EditorState::new();
    state.chat.chosen_style = Some(chosen(style_ref));
    state
}

#[test]
fn loop_context_block_includes_the_chosen_style_and_is_capped() {
    if op_ai_skills::style_by_ref("glassmorphism").is_none() {
        return; // no embedded style table on this target
    }
    let state = with_choice("glassmorphism");
    let block = design_user_context_block(&state, None).expect("a choice alone makes a block");
    assert!(block.contains("### Style chosen for this run"));
    assert!(block.contains(CHOSEN_STYLE_MARKER));
    assert!(block.contains("Glassmorphism"));
    assert!(block.contains("Best for:"));
    let paragraph = block
        .split("### Style chosen for this run\n")
        .nth(1)
        .expect("section");
    assert!(
        paragraph.len() <= MAX_CHOSEN_STYLE_BYTES,
        "paragraph is {} bytes",
        paragraph.len()
    );
    // It rides the per-run block, after the static skills.
    let prompt = design_loop_system_prompt(&state, PROMPT, None);
    assert!(prompt.contains(CHOSEN_STYLE_MARKER));
}

#[test]
fn loop_prompt_is_byte_identical_without_a_choice() {
    // The block with no choice, no design.md, no pin and no selection does not
    // exist, and the prompt is exactly the one built from its two parts.
    let state = EditorState::new();
    assert!(state.chat.chosen_style.is_none());
    assert!(design_user_context_block(&state, None).is_none());
    let mut expected = op_ai_skills::design_agent_system_prompt_with_skills(PROMPT);
    if let Some(brief) = crate::design_context::design_context_brief(&state) {
        expected.push_str("\n\n---\n\n");
        expected.push_str(&brief);
        op_ai_skills::append_image_self_check_scope(&mut expected);
    }
    assert_eq!(design_loop_system_prompt(&state, PROMPT, None), expected);

    // Choosing and then clearing leaves no trace.
    let mut toggled = with_choice("glassmorphism");
    toggled.chat.chosen_style = None;
    assert_eq!(design_loop_system_prompt(&toggled, PROMPT, None), expected);
}

#[test]
fn pinned_style_and_design_md_outrank_the_choice_in_the_text() {
    if op_ai_skills::style_by_ref("glassmorphism").is_none() {
        return;
    }
    let mut state = with_choice("glassmorphism");
    state.doc.design_md = Some(DesignMdSpec {
        raw: String::new(),
        project_name: None,
        visual_theme: Some("Brutalist".into()),
        color_palette: None,
        typography: None,
        component_styles: None,
        layout_principles: None,
        generation_notes: None,
    });
    let block = design_user_context_block(&state, None).expect("block");
    let design_md = block.find("VISUAL THEME: Brutalist").expect("design.md");
    let choice = block.find("### Style chosen for this run").expect("choice");
    assert!(design_md < choice, "the weaker decision comes after");
    assert!(block.contains("design.md and the pinned style (if any) outrank it"));

    let mut pinned = with_choice("glassmorphism");
    pinned.editor_ui.pinned_style_guide = Some("dark-bold-mobile".into());
    let block = design_user_context_block(&pinned, None).expect("block");
    let pin = block.find("### Pinned style guide").expect("pin");
    let choice = block.find("### Style chosen for this run").expect("choice");
    assert!(pin < choice);
}

#[test]
fn a_choice_the_table_no_longer_knows_adds_nothing() {
    let mut state = EditorState::new();
    state.chat.chosen_style = Some(StyleSuggestion {
        style_ref: "retired-style".into(),
        name: "Retired".into(),
        blurb: String::new(),
    });
    assert!(design_user_context_block(&state, None).is_none());
}
