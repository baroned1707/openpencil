//! The one model-free prompt check: render the design-agent system prompt for
//! an eval case's prompt and record which skills end up in it and what it
//! costs. The gate pins the skill names and a token ceiling; it never pins
//! prompt text.

use op_ai_skills::budget::estimate_tokens;
use op_ai_skills::{design_agent_system_prompt_with_skills, resolve_skills, Phase, ResolveOptions};
use serde::{Deserialize, Serialize};

/// What rendering the system prompt for one case's prompt gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptRender {
    /// Resolved generation-phase skills whose text is inside the rendered
    /// prompt, sorted by name.
    pub skills: Vec<String>,
    /// `estimate_tokens` (characters / 4) of the whole rendered prompt.
    pub token_estimate: u32,
}

pub(crate) fn render_prompt(user_prompt: &str) -> PromptRender {
    let rendered = design_agent_system_prompt_with_skills(user_prompt);
    let context = resolve_skills(Phase::Generation, user_prompt, &ResolveOptions::default());
    let mut skills: Vec<String> = context
        .skills
        .iter()
        .filter(|skill| {
            let body = skill.content.trim();
            !body.is_empty() && rendered.contains(body)
        })
        .map(|skill| skill.meta.name.clone())
        .collect();
    skills.sort();
    skills.dedup();
    PromptRender {
        skills,
        token_estimate: estimate_tokens(&rendered),
    }
}
