//! Prompt routing and wording guards for `design-vector-drawing`: it loads
//! for requests to draw icons, logos and illustrations, and stays out of
//! ordinary edit and screen prompts.

use crate::{get_skill_by_name, resolve_skills, Phase, ResolveOptions};

const SKILL: &str = "design-vector-drawing";

fn loads(prompt: &str) -> bool {
    resolve_skills(Phase::Generation, prompt, &ResolveOptions::default())
        .skills
        .iter()
        .any(|skill| skill.meta.name == SKILL)
}

fn text() -> String {
    get_skill_by_name(SKILL)
        .expect("design-vector-drawing is registered")
        .content
        .clone()
}

#[test]
fn loads_for_drawing_requests() {
    for prompt in [
        "draw a logo for a coffee shop",
        "design a set of 12 icons",
        "create an SVG illustration",
        "Design a mascot for a bakery",
        "make an icon set for a travel site",
        "draw an icon of a paper plane",
        "import this svg and clean it up",
        "设计一套图标设计",
        "画一幅插画",
    ] {
        assert!(loads(prompt), "{prompt:?} should load {SKILL}");
    }
}

#[test]
fn stays_out_of_edit_and_screen_prompts() {
    for prompt in [
        "make the button blue",
        "change the header colour to green",
        "resize the card and fix the spacing",
        "Design a mobile fitness app home screen",
        "Design an analytics dashboard with a client table",
        "Design a landing page for a specialty coffee roaster",
        "add a settings page with a toggle for notifications",
    ] {
        assert!(!loads(prompt), "{prompt:?} must not load {SKILL}");
    }
}

#[test]
fn teaches_the_tool_specific_rules() {
    let text = text();
    for required in [
        "icon_font",
        "iconIssues",
        "layoutIssues",
        "get_screenshot",
        "scale 8-16",
        "import_svg",
        "skipped",
        "gradient",
        "at most 2 correction rounds",
        "never trace",
    ] {
        assert!(
            text.to_lowercase().contains(&required.to_lowercase()),
            "missing {required:?}"
        );
    }
}
