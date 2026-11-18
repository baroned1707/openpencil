//! Invariants of the in-app design loop's system prompt text.

use std::collections::HashSet;

use op_ai_skills::{DesignVerifyProtocol, PromptSurface};

use crate::design_agent_tools::DESIGN_TOOLS;
use crate::tool_schemas::TOOL_SCHEMAS;

const PROMPTS: [&str; 3] = [
    "design a mobile app login screen",
    "build a saas landing page for a design tool",
    "admin dashboard with analytics and a client table",
];

/// Names of every tool the full MCP catalog advertises.
fn catalog_tool_names() -> Vec<String> {
    TOOL_SCHEMAS
        .iter()
        .map(|entry| {
            let value: serde_json::Value = serde_json::from_str(entry).expect("schema json");
            value["name"].as_str().expect("tool name").to_string()
        })
        .collect()
}

fn loop_prompts() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for prompt in PROMPTS {
        for verify in [
            DesignVerifyProtocol::Screenshot,
            DesignVerifyProtocol::LayoutSnapshot,
        ] {
            out.push((
                format!("{prompt} / {verify:?}"),
                op_ai_skills::design_agent_system_prompt_with_skills_for(prompt, verify),
            ));
        }
    }
    out
}

#[test]
fn loop_text_never_tells_the_model_to_call_finalize_design() {
    for (label, prompt) in loop_prompts() {
        assert!(
            !prompt.contains("finalize_design"),
            "loop system prompt ({label}) mentions finalize_design, a tool the loop lacks"
        );
        assert!(
            prompt.contains("The host finalizes the design when your loop ends"),
            "loop system prompt ({label}) must say who finalizes"
        );
    }
    for topic in op_ai_skills::guideline_topics() {
        let text = op_ai_skills::guideline_for(topic).expect("listed topic resolves");
        assert!(
            !text.contains("finalize_design"),
            "in-app guideline {topic:?} mentions finalize_design"
        );
    }
}

#[test]
fn every_tool_named_in_the_loop_prompt_exists_in_design_tools() {
    let loop_tools: HashSet<&str> = DESIGN_TOOLS.iter().map(|(name, _)| *name).collect();
    // Whole-word match on the catalog's snake_case names; plain English words
    // that happen to be tool names are not looked for (none contain `_`).
    let catalog: Vec<String> = catalog_tool_names()
        .into_iter()
        .filter(|name| name.contains('_'))
        .collect();
    let mut offenders: Vec<String> = Vec::new();
    for (label, prompt) in loop_prompts() {
        let words: HashSet<&str> = prompt
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| !word.is_empty())
            .collect();
        for name in &catalog {
            if words.contains(name.as_str()) && !loop_tools.contains(name.as_str()) {
                offenders.push(format!("{name} in [{label}]"));
            }
        }
    }
    offenders.sort();
    offenders.dedup();
    assert!(
        offenders.is_empty(),
        "loop prompt names tools the loop is not given: {offenders:#?}"
    );
}

#[test]
fn the_mcp_surface_keeps_the_finalize_instruction() {
    for verify in [
        DesignVerifyProtocol::Screenshot,
        DesignVerifyProtocol::LayoutSnapshot,
    ] {
        let prompt = op_ai_skills::design_agent_system_prompt_with_skills_for_surface(
            PROMPTS[0],
            verify,
            PromptSurface::Mcp,
        );
        assert!(prompt.contains("call `finalize_design` once"));
        assert!(!prompt.contains("The host finalizes the design"));
    }
}

// ---------------------------------------------------------------------------
// The user's design decisions in the loop prompt
// ---------------------------------------------------------------------------

use crate::design_loop_prompt::{
    design_loop_system_prompt, design_user_context_block, DESIGN_MD_MAX_BYTES, SELECTION_MAX_NODES,
};
use jian_ops_schema::node::PenNode;
use jian_ops_schema::DesignMdSpec;
use op_editor_core::{EditorState, NodeId};

fn design_md(theme: &str) -> DesignMdSpec {
    DesignMdSpec {
        raw: String::new(),
        project_name: None,
        visual_theme: Some(theme.into()),
        color_palette: None,
        typography: None,
        component_styles: None,
        layout_principles: None,
        generation_notes: None,
    }
}

/// A screen `home` holding `count` selectable cards `card0..`.
fn state_with_cards(count: usize) -> EditorState {
    let cards: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "type": "frame", "id": format!("card{i}"), "name": format!("Card {i}"),
                "width": 100, "height": 50
            })
        })
        .collect();
    let home: PenNode = serde_json::from_value(serde_json::json!({
        "type": "frame", "id": "home", "name": "Home", "width": 390, "height": 844,
        "layout": "vertical", "children": cards
    }))
    .expect("home");
    let mut state = EditorState::new();
    state
        .insert_subtree_returning_root_ids(vec![home], &NodeId::NONE)
        .expect("insert");
    state
}

/// The id the editor assigned to the node named `name` (inserts remap ids).
fn id_of(state: &EditorState, name: &str) -> String {
    fn walk(nodes: &[PenNode], name: &str) -> Option<String> {
        use op_editor_core::pen_node_ext::PenNodeExt;
        for node in nodes {
            if node.base().name.as_deref() == Some(name) {
                return Some(node.id_str().to_string());
            }
            if let Some(hit) = node.children().and_then(|c| walk(c, name)) {
                return Some(hit);
            }
        }
        None
    }
    walk(state.active_children(), name).expect("named node")
}

fn select(state: &mut EditorState, names: &[&str]) {
    let ids: Vec<String> = names.iter().map(|name| id_of(state, name)).collect();
    state.selection.set = ids.into_iter().map(NodeId::new).collect();
    state.selection.anchor = state.selection.set.last().cloned().unwrap_or(NodeId::NONE);
}

/// The prompt as the desktop built it before the context block existed.
fn legacy_prompt(state: &EditorState, user_text: &str) -> String {
    let mut prompt = op_ai_skills::design_agent_system_prompt_with_skills(user_text);
    if let Some(brief) = crate::design_context::design_context_brief(state) {
        prompt.push_str("\n\n---\n\n");
        prompt.push_str(&brief);
        op_ai_skills::append_image_self_check_scope(&mut prompt);
    }
    prompt
}

#[test]
fn loop_prompt_is_byte_identical_without_context() {
    let empty = EditorState::new();
    assert_eq!(
        design_loop_system_prompt(&empty, PROMPTS[0], None),
        legacy_prompt(&empty, PROMPTS[0])
    );
    // A populated canvas with no design.md / pin / selection: only the brief.
    let populated = state_with_cards(2);
    assert!(design_user_context_block(&populated, None).is_none());
    assert_eq!(
        design_loop_system_prompt(&populated, PROMPTS[1], None),
        legacy_prompt(&populated, PROMPTS[1])
    );
    // An empty-string pin and a NONE selection count as nothing.
    let mut blank = state_with_cards(1);
    blank.editor_ui.pinned_style_guide = Some("  ".into());
    assert!(design_user_context_block(&blank, None).is_none());
}

#[test]
fn loop_prompt_includes_design_md_pin_and_selection() {
    // design.md + selection.
    let mut state = state_with_cards(3);
    state.doc.design_md = Some(design_md("Calm editorial paper"));
    select(&mut state, &["Card 0", "Card 2"]);
    let prompt = design_loop_system_prompt(&state, PROMPTS[0], None);
    let skills = op_ai_skills::design_agent_system_prompt_with_skills(PROMPTS[0]);
    let static_len = skills
        .rfind("\n\n---\n\n## Image self-check scope")
        .expect("scope tail");
    assert!(
        prompt.starts_with(&skills[..static_len]),
        "static prefix first"
    );
    assert!(prompt.contains("VISUAL THEME: Calm editorial paper"));
    assert!(prompt.contains("decisions: follow them"));
    let (card0, card2) = (id_of(&state, "Card 0"), id_of(&state, "Card 2"));
    assert!(prompt.contains(&format!("- {card0} \"Card 0\" (frame, 100x50)")));
    assert!(prompt.contains(&format!("- {card2} \"Card 2\" (frame, 100x50)")));
    assert!(!prompt.contains("\"Card 1\""), "unselected node stays out");
    // The per-run block sits after the static skills and before the canvas brief.
    let block = prompt.find("The user's design decisions").unwrap();
    let brief = prompt.find("EXISTING CANVAS CONTEXT").unwrap();
    assert!(block > static_len && block < brief);
    // The image self-check scope is still the authoritative tail.
    assert!(prompt.ends_with(op_ai_skills::IMAGE_SELF_CHECK_SCOPE));

    // Pin + selection (no design.md): the pin block, named by id.
    let mut pinned = state_with_cards(2);
    pinned.editor_ui.pinned_style_guide = Some("dark-bold-mobile".into());
    select(&mut pinned, &["Home"]);
    let block = design_user_context_block(&pinned, None).expect("block");
    assert!(block.contains("### Pinned style guide: dark-bold-mobile"));
    let home = id_of(&pinned, "Home");
    assert!(block.contains(&format!("- {home} \"Home\" (frame, 390x844)")));
    assert!(block.len() < 6 * 1024 + 2048, "pin block is bounded");

    // design.md outranks the pin, as in the orchestrator.
    pinned.doc.design_md = Some(design_md("Brutalist"));
    let block = design_user_context_block(&pinned, None).expect("block");
    assert!(block.contains("VISUAL THEME: Brutalist"));
    assert!(!block.contains("Pinned style guide"));

    // A stale pin contributes nothing.
    let mut stale = state_with_cards(1);
    stale.editor_ui.pinned_style_guide = Some("no-such-guide-anywhere".into());
    assert!(design_user_context_block(&stale, None).is_none());
}

#[test]
fn design_md_is_truncated_with_a_marker() {
    let mut state = EditorState::new();
    let mut spec = design_md("");
    spec.visual_theme = None;
    // Free-form design.md: no structured fields, so the raw text is used.
    spec.raw = format!("{}é", "x".repeat(DESIGN_MD_MAX_BYTES * 2));
    state.doc.design_md = Some(spec);
    let block = design_user_context_block(&state, None).expect("block");
    assert!(block.ends_with("[design.md truncated]"));
    assert!(
        block.len() < DESIGN_MD_MAX_BYTES + 700,
        "bounded: {}",
        block.len()
    );

    // A short design.md is carried whole, with no marker.
    let mut short = EditorState::new();
    let mut spec = design_md("");
    spec.visual_theme = None;
    spec.raw = "Use the brand blue.".into();
    short.doc.design_md = Some(spec);
    let block = design_user_context_block(&short, None).expect("block");
    assert!(block.contains("Use the brand blue."));
    assert!(!block.contains("truncated"));
}

#[test]
fn selection_lists_at_most_twenty_nodes_and_counts_the_rest() {
    let mut state = state_with_cards(25);
    let ids: Vec<String> = (0..25).map(|i| format!("Card {i}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    select(&mut state, &refs);
    let block = design_user_context_block(&state, None).expect("block");
    assert_eq!(block.matches("\n- ").count(), SELECTION_MAX_NODES + 1);
    assert!(block.contains("...and 5 more selected"));
    assert!(block.contains("Current selection (25 node(s)"));
}

#[test]
fn the_loop_prompt_text_is_stable_between_builds() {
    // Anthropic caches on the exact prefix. Rebuilding must reproduce the same
    // bytes (a HashMap iteration or timestamp leaking in would not).
    let mut state = state_with_cards(2);
    state.doc.design_md = Some(design_md("Calm"));
    select(&mut state, &["Card 1"]);
    let first = design_loop_system_prompt(&state, PROMPTS[0], None);
    for _ in 0..4 {
        assert_eq!(design_loop_system_prompt(&state, PROMPTS[0], None), first);
    }
    let tools = |_: ()| -> String {
        let defs = crate::design_agent_tools::design_tool_defs();
        defs.iter()
            .map(|t| format!("{}|{}|{}", t.name, t.description, t.input_schema_json))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(tools(()), tools(()));
}

#[test]
fn in_app_get_guidelines_has_no_finalize_instruction() {
    let mut state = EditorState::new();
    let (result, _) = crate::design_agent_tools::execute_agent_tool(
        &mut state,
        "get_guidelines",
        r#"{"topic":"mobile"}"#,
    );
    assert!(!result.is_error, "{}", result.content);
    assert!(result.content.contains("THREE-SECTION ARCHITECTURE"));
    assert!(!result.content.contains("finalize_design"));
}
