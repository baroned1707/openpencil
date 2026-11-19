//! Invariants of the four design-intelligence skills (`design-craft`,
//! `design-brief`, `design-wcag-quickcheck`, `design-vector-drawing`): they reach the in-app loop
//! without displacing the domain skills that were already there, stay small,
//! and carry valid frontmatter and no platform-guideline material.

use crate::budget::estimate_tokens;
use crate::resolver::match_keyword;
use crate::{
    design_agent_system_prompt_with_skills, design_agent_system_prompt_with_skills_for_surface,
    get_skill_by_name, resolve_skills, DesignVerifyProtocol, DropReason, Phase, PromptSurface,
    ResolveOptions, SkillCategory,
};

const NEW_SKILLS: [&str; 4] = [
    "design-craft",
    "design-brief",
    "design-wcag-quickcheck",
    "design-vector-drawing",
];

/// Growth the four skills may add to a loop prompt (tokens, chars / 4).
const MAX_LOOP_GROWTH_TOKENS: u32 = 2_400;

/// (prompt, domain skill that must keep loading, a heading of that skill).
const LOOP_CASES: [(&str, &str, &str); 3] = [
    (
        "Design a mobile fitness app home screen",
        "mobile-app",
        "THREE-SECTION ARCHITECTURE",
    ),
    (
        "Design an analytics dashboard with a client table",
        "dashboard",
        "DASHBOARD / ADMIN / DATA-TABLE DEPTH",
    ),
    (
        "Design a landing page for a specialty coffee roaster",
        "landing-page",
        "LANDING PAGE — CONVERSION-DRIVEN MARKETING DEPTH",
    ),
];

const CRAFT_HEADING: &str = "CRAFT FLOOR AND SELF-REVIEW";

fn skill_text(name: &str) -> String {
    get_skill_by_name(name)
        .unwrap_or_else(|| panic!("{name} must be registered"))
        .content
        .clone()
}

#[test]
fn design_craft_reaches_the_loop_for_mobile_dashboard_and_landing() {
    for (prompt, _, domain_heading) in LOOP_CASES {
        let loop_prompt = design_agent_system_prompt_with_skills(prompt);
        assert!(
            loop_prompt.contains(CRAFT_HEADING),
            "loop prompt for {prompt:?}"
        );
        assert!(loop_prompt.contains(domain_heading), "{prompt:?}");
        // The MCP tool `get_design_agent_prompt` renders the same function.
        for verify in [
            DesignVerifyProtocol::Screenshot,
            DesignVerifyProtocol::LayoutSnapshot,
        ] {
            let external = design_agent_system_prompt_with_skills_for_surface(
                prompt,
                verify,
                PromptSurface::Mcp,
            );
            assert!(external.contains(CRAFT_HEADING), "{prompt:?} / {verify:?}");
        }
    }
}

#[test]
fn existing_domain_skills_still_load_untruncated() {
    let heavy = "Design a mobile banking app dashboard and landing page";
    let prompts = LOOP_CASES
        .iter()
        .map(|(prompt, _, _)| *prompt)
        .chain([heavy]);
    for prompt in prompts {
        let context = resolve_skills(Phase::Generation, prompt, &ResolveOptions::default());
        let starved: Vec<&str> = context
            .report
            .dropped
            .iter()
            .filter(|dropped| dropped.reason == DropReason::BudgetExhausted)
            .map(|dropped| dropped.name.as_str())
            .collect();
        assert!(
            starved.is_empty(),
            "{prompt:?} dropped for budget: {starved:?}"
        );
        let truncated: Vec<&str> = context
            .skills
            .iter()
            .filter(|skill| skill.truncated)
            .map(|skill| skill.meta.name.as_str())
            .collect();
        assert!(truncated.is_empty(), "{prompt:?} truncated: {truncated:?}");
    }
    for (prompt, domain, _) in LOOP_CASES {
        let context = resolve_skills(Phase::Generation, prompt, &ResolveOptions::default());
        let loaded = context
            .skills
            .iter()
            .find(|skill| skill.meta.name == domain)
            .unwrap_or_else(|| panic!("{domain} no longer loads for {prompt:?}"));
        assert_eq!(loaded.content, skill_text(domain), "{domain} lost bytes");
    }
}

#[test]
fn design_brief_triggers_for_new_product_prompts_not_for_modify_prompts() {
    let loads = |prompt: &str| {
        resolve_skills(Phase::Generation, prompt, &ResolveOptions::default())
            .skills
            .iter()
            .any(|skill| skill.meta.name == "design-brief")
    };
    for prompt in [
        "Design a mobile fitness app home screen",
        "Create an app for tracking houseplants",
        "Build a website for a ceramics studio",
        "make me a booking app from scratch",
        // "an" before a vowel: the word-boundary matcher needs its own keyword.
        "make me an app screen",
        "设计一个记账应用",
    ] {
        assert!(loads(prompt), "{prompt:?} should load design-brief");
    }
    for prompt in [
        "make the button blue",
        "change the header colour to green",
        "resize the card and fix the spacing",
        "update the title text",
        "remove the sidebar",
        "make an icon bigger",
        "move the logo to the left",
        "add to the cart section a coupon field",
        "让按钮变成蓝色",
    ] {
        assert!(!loads(prompt), "{prompt:?} must not load design-brief");
    }
}

#[test]
fn the_tool_loop_only_list_is_exactly_the_new_skills() {
    // The orchestrator sub-agents, the plain chat prompt and the builtin
    // preamble all drop skills by this one list.
    assert_eq!(crate::TOOL_LOOP_ONLY_SKILLS, NEW_SKILLS);
}

#[test]
fn design_brief_keeps_the_brief_next_to_a_tool_call() {
    // A reply with no tool call ends the loop: the brief must never be a turn
    // of its own.
    let text = skill_text("design-brief");
    assert!(text.contains("Never send it as a reply with no tool call"));
}

#[test]
fn design_brief_names_its_tool_call() {
    let text = skill_text("design-brief");
    assert!(!text.contains("Step 3"), "no dangling reference to a step");
    assert!(text.contains("get_guidelines") && text.contains("category"));
    assert!(
        text.contains("\"data\""),
        "the data category is spelled out"
    );
}

#[test]
fn design_brief_mentions_kind_style() {
    // The one sentence that tells the loop how to read a style named in a
    // product match, and the skill still fits its own 1,100-token budget.
    let text = skill_text("design-brief");
    assert!(text.contains(
        "To read a style named in a match, call `get_guidelines` with category data, kind style and the style name."
    ));
    assert!(!text.contains("Step 3"));
    assert!(
        estimate_tokens(&text) <= 1_100,
        "{} tokens",
        estimate_tokens(&text)
    );
}

#[test]
fn loop_prompt_growth_is_within_budget() {
    // Static bound: all four skills together, were every one to load.
    let all_four: u32 = NEW_SKILLS
        .iter()
        .map(|name| estimate_tokens(&skill_text(name)))
        .sum();
    assert!(all_four <= MAX_LOOP_GROWTH_TOKENS, "{all_four} tokens");
    // Per prompt: render, remove the new skills' text, compare.
    for (prompt, _, _) in LOOP_CASES {
        let with = design_agent_system_prompt_with_skills(prompt);
        let mut without = with.clone();
        let mut loaded = 0;
        for name in NEW_SKILLS {
            let text = skill_text(name);
            if without.contains(text.trim()) {
                without = without.replace(text.trim(), "");
                loaded += 1;
            }
        }
        assert!(loaded >= 2, "{prompt:?}: craft and brief should both load");
        let growth = estimate_tokens(&with) - estimate_tokens(&without);
        assert!(
            growth <= MAX_LOOP_GROWTH_TOKENS,
            "{prompt:?}: the new skills grow the loop prompt by {growth} tokens"
        );
    }
}

#[test]
fn wcag_quickcheck_has_no_w3c_verbatim_text() {
    // Cheap guard: sentence fragments from the W3C normative text and its
    // Understanding pages. The skill is an independent summary by number.
    let text = skill_text("design-wcag-quickcheck").to_lowercase();
    for fragment in [
        "success criterion",
        "understanding wcag",
        "has a contrast ratio of at least",
        "the size of the target for pointer inputs",
        "except for the following",
        "can be programmatically determined",
        "color is not used as the only visual means",
        "images of text",
        "a mechanism is available",
        "w3c",
    ] {
        assert!(!text.contains(fragment), "W3C wording: {fragment:?}");
    }
}

/// The frontmatter block of an embedded skill file (between the `---` rules).
fn frontmatter_of(relative: &str) -> String {
    let file = crate::SKILLS
        .get_file(relative)
        .unwrap_or_else(|| panic!("{relative} is embedded"));
    let text = file.contents_utf8().expect("utf-8");
    let rest = text.strip_prefix("---\n").expect("frontmatter opens");
    rest[..rest.find("\n---").expect("frontmatter closes")].to_string()
}

#[test]
fn skill_frontmatter_is_valid() {
    for name in NEW_SKILLS {
        let raw = frontmatter_of(&format!("domains/{name}.md"));
        // `#` outside quotes starts a comment in the hand-rolled parser, so a
        // hex colour in frontmatter would silently cut the line.
        assert!(!raw.contains('#'), "{name}: no '#' in frontmatter");
        for key in [
            "name:",
            "phase:",
            "category:",
            "budget:",
            "keywords:",
            "priority:",
        ] {
            assert!(raw.contains(key), "{name}: frontmatter lacks {key}");
        }
        let entry = get_skill_by_name(name).expect("parsed, not silently dropped");
        assert_eq!(entry.meta.name, name);
        assert_eq!(entry.meta.phase, vec![Phase::Generation]);
        assert_eq!(entry.meta.category, SkillCategory::Domain);
        assert!(entry.meta.budget > 0);
        assert!(
            estimate_tokens(&entry.content) <= entry.meta.budget,
            "{name} exceeds its own budget"
        );
        assert!(!entry.meta.description.trim().is_empty());
    }
    // Their budgets are what the report states, and the sum stays bounded.
    let budgets: Vec<u32> = NEW_SKILLS
        .iter()
        .map(|name| get_skill_by_name(name).expect("registered").meta.budget)
        .collect();
    assert_eq!(budgets, [1300, 1100, 500, 650]);
}

#[test]
fn no_apple_or_ios_android_reference_material_imported() {
    for name in NEW_SKILLS {
        let text = skill_text(name).to_lowercase();
        for term in [
            "human interface",
            "hig",
            "swiftui",
            "uikit",
            "material design",
            "ios.md",
            "android.md",
            "dynamic type",
        ] {
            assert!(
                !match_keyword(&text, term),
                "{name} mentions platform-guideline material: {term:?}"
            );
        }
    }
}
