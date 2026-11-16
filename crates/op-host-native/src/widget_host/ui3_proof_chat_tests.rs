//! Proof scenes for the Figma UI3 restyle — chat wave.
//!
//! Add one `#[test]` per surface; each calls `ui3_proof::render_scene` once
//! per theme so `.proof-of-work/` gets a dark and a light PNG.

use crate::widget_host::ui3_proof::{editor_host, render_scene};
use crate::widget_host::WidgetHostNative;
use op_editor_core::chat::{AgentProvider, ChatMessage, ChatToolCall, ModelEntry};
use op_editor_core::{ChatActivity, ChatActivityStatus, ThemeMode};

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// An editor host with the rail on the Agent tab and a model catalog, so
/// the composer shows a real model chip.
fn chat_host(theme: ThemeMode) -> WidgetHostNative {
    let mut host = editor_host(theme, None);
    let state = host.editor_state_mut();
    state.editor_ui.enter_chat_tab();
    state.chat.available_models = vec![
        ModelEntry::new(
            AgentProvider::ClaudeCode,
            "claude-sonnet-4-6",
            "Claude Sonnet 4.6",
        ),
        ModelEntry::new(
            AgentProvider::ClaudeCode,
            "claude-opus-4-6",
            "Claude Opus 4.6",
        ),
        ModelEntry::new(AgentProvider::CodexCli, "gpt-5.5", "GPT-5.5"),
        ModelEntry::new(AgentProvider::CodexCli, "gpt-5.5-mini", "GPT-5.5 Mini"),
    ];
    state.chat.selected_model = 0;
    host
}

fn tool_call(name: &str, args: &str, offset: u32) -> ChatToolCall {
    ChatToolCall {
        name: name.to_string(),
        args: args.to_string(),
        content_offset: Some(offset),
    }
}

/// A user brief, then a streaming assistant turn that mixes narration with
/// running / done / failed tool steps, design-progress activities and a
/// still-open tail.
fn seed_transcript(host: &mut WidgetHostNative) {
    let chat = &mut host.editor_state_mut().chat;
    chat.messages.push(ChatMessage::user(
        "Design a dashboard card with a metric and a primary button.",
    ));
    let narration = "I'll read the page first, then lay out the card.\n\n";
    let tail = "Next I will adjust the spacing and check the contrast.";
    let at = narration.len() as u32;
    let mut assistant = ChatMessage::assistant_streaming();
    assistant.agent_name = Some("Claude Code".to_string());
    assistant.content = format!("{narration}{tail}");
    assistant.tools_collapsed = false;
    assistant.tool_calls = vec![
        tool_call(
            "get_selection",
            r#"{"status":"done","result":{"success":true,"data":{"count":0}}}"#,
            0,
        ),
        tool_call(
            "batch_design",
            r#"{"status":"running","args":{"parent":"board"}}"#,
            at,
        ),
        tool_call(
            "update_node",
            r#"{"status":"error","result":{"success":false,"error":"node not found: card-2"}}"#,
            at,
        ),
    ];
    assistant.activities = vec![
        activity(
            "a1",
            "Plan layout",
            Some("3 sections"),
            ChatActivityStatus::Done,
            at,
        ),
        activity(
            "a2",
            "Draw metric card",
            None,
            ChatActivityStatus::Running,
            at,
        ),
        activity(
            "a3",
            "Add primary button",
            Some("Failed to resolve parent"),
            ChatActivityStatus::Error,
            at,
        ),
    ];
    chat.messages.push(assistant);
}

fn activity(
    id: &str,
    title: &str,
    detail: Option<&str>,
    status: ChatActivityStatus,
    offset: u32,
) -> ChatActivity {
    ChatActivity {
        id: id.into(),
        title: title.into(),
        detail: detail.map(str::to_string),
        status,
        content_offset: Some(offset),
    }
}

/// A finished assistant turn carrying a design block, expanded so the code
/// well and the Apply button both paint.
fn seed_design_apply(host: &mut WidgetHostNative) {
    let chat = &mut host.editor_state_mut().chat;
    chat.messages
        .push(ChatMessage::user("Add a metric card to the board."));
    let mut done = ChatMessage::assistant(
        "Here is the card:\n\n```json\n[{\"type\":\"frame\",\"id\":\"metric-card\",\"name\":\"Metric Card\",\"x\":0,\"y\":0,\"width\":280,\"height\":160,\"children\":[]}]\n```",
    );
    done.agent_name = Some("Claude Code".to_string());
    done.design_block_expanded_overrides = vec![Some(true)];
    chat.messages.push(done);
}

const STYLE_GUIDE: &str = "---
name: Dimension
---

## Overview

A dark reference system built around one violet accent.

## Tokens — Colors

| Name | Value | Token | Role |
| --- | --- | --- | --- |
| Void Canvas | `#0a0a0a` | `--color-void-canvas` | Primary page background |
| Graphite | `#161616` | `--color-graphite` | Elevated surface |
| Bone | `#ededed` | `--color-bone` | Primary readable text |
| Violet | `#7c5cff` | `--color-violet` | Accent |
";

/// Pin an imported style guide so the style chip paints above the composer.
fn pin_style(host: &mut WidgetHostNative) {
    let imported = op_ai_skills::style_guide::import_design_md(STYLE_GUIDE, "dimension.md")
        .expect("style guide imports");
    host.editor_state_mut().editor_ui.pinned_style_guide = Some(imported.id.clone());
}

/// Pin a style guide and let the hover dwell elapse so the chip's detail
/// card paints too.
fn pin_style_with_card(host: &mut WidgetHostNative) {
    pin_style(host);
    host.editor_state_mut()
        .editor_ui
        .set_chat_style_chip_hover(true, 0);
    host.now_ms = 10_000;
}

/// Antigravity hard-requires the MCP integration, so selecting it without
/// the toggle raises the pre-flight notice above the composer.
fn select_antigravity(host: &mut WidgetHostNative) {
    host.editor_state_mut().editor_ui.chat_selected_agent = AgentProvider::ALL
        .iter()
        .position(|p| *p == AgentProvider::Antigravity)
        .unwrap();
}

fn render_both(name: &str, mut setup: impl FnMut(&mut WidgetHostNative)) {
    for theme in THEMES {
        let mut host = chat_host(theme);
        setup(&mut host);
        let rgb = render_scene(&mut host, name, (60, 400));
        assert!(rgb != [0, 0, 0], "{theme:?}: {name} painted something");
    }
}

#[test]
fn chat_empty_renders() {
    render_both("20-chat-empty", |_| {});
}

#[test]
fn chat_transcript_renders() {
    render_both("20-chat-transcript", |host| {
        seed_transcript(host);
        host.editor_state_mut()
            .chat
            .set_input_text("Make the button rounder");
        select_antigravity(host);
        pin_style(host);
    });
}

#[test]
fn chat_design_apply_renders() {
    render_both("20-chat-design-apply", |host| {
        seed_design_apply(host);
    });
}

#[test]
fn chat_style_card_renders() {
    render_both("20-chat-style-card", |host| {
        seed_transcript(host);
        pin_style_with_card(host);
    });
}

#[test]
fn chat_mcp_notice_renders() {
    render_both("20-chat-mcp-notice", |host| {
        seed_transcript(host);
        select_antigravity(host);
    });
}

#[test]
fn chat_model_picker_renders() {
    render_both("20-chat-model-picker", |host| {
        seed_transcript(host);
        host.editor_state_mut().editor_ui.chat_model_picker.open = true;
    });
}

#[test]
fn chat_floating_composer_renders() {
    // Off the Agent tab the chat is the composer card floating over the
    // canvas floor — the surface that separates by shadow, not border.
    render_both("20-chat-floating", |host| {
        host.editor_state_mut().editor_ui.slides_panel.tab = op_editor_core::LeftPanelTab::Layers;
        host.editor_state_mut()
            .chat
            .set_input_text("Describe what to design");
    });
}
