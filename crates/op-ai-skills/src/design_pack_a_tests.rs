//! Pins for the "Pack A" prompt rules: the pre-build generic-default check,
//! the content inventory, the screenshot cadence (one wording in three
//! places), the Done / Remaining / Unsure summary and the re-plan sentence.
//! Presence and size only: nothing here checks that a model follows a rule.

use crate::budget::estimate_tokens;
use crate::{
    design_agent_system_prompt, design_agent_system_prompt_for, get_skill_by_name,
    DesignVerifyProtocol, SCREENSHOT_FINISH_CONDITION, SCREENSHOT_VERIFY_STEP,
};

/// The one cadence the three screenshot texts must share, compared with
/// backticks removed (the skill text writes tool names bare).
const CADENCE: &str = "one full get_screenshot after the first complete build";

/// `design-agent.md` before Pack A (bytes). The summary rule replaces an older
/// sentence, so the file may only shrink: its 700-byte pin stays where it is.
const DESIGN_AGENT_BYTES_BEFORE_PACK_A: usize = 30_241;

fn skill_text(name: &str) -> String {
    get_skill_by_name(name)
        .unwrap_or_else(|| panic!("{name} must be registered"))
        .content
        .clone()
}

fn plain(text: &str) -> String {
    text.replace('`', "")
}

#[test]
fn the_three_screenshot_texts_say_the_same_thing() {
    let texts = [
        ("design-craft", plain(&skill_text("design-craft"))),
        ("verify step", plain(SCREENSHOT_VERIFY_STEP)),
        ("finish condition", plain(SCREENSHOT_FINISH_CONDITION)),
    ];
    for (name, text) in &texts {
        assert!(text.contains(CADENCE), "{name} lacks the cadence: {text}");
        // The old, contradicting wording is gone from every one of them.
        for old in [
            "at most one get_screenshot",
            "until the screenshot reads",
            "Iterate",
        ] {
            assert!(!text.contains(old), "{name} still says {old:?}");
        }
    }
    // The cap on extra screenshots is stated where the model fixes things.
    assert!(texts[0].1.contains("take at most one more"));
    assert!(texts[1].1.contains("take at most one more"));
    assert!(texts[2].1.contains("at most one more"));
    // The final screenshot stays mandatory; only the cadence changed.
    assert!(texts[1].1.contains("mandatory, not optional"));
    assert!(texts[1]
        .1
        .contains("do not declare the design done without it"));
}

#[test]
fn the_screenshot_prompt_carries_the_cadence_and_the_layout_prompt_does_not() {
    let screenshot = plain(design_agent_system_prompt());
    assert_eq!(
        screenshot.matches(CADENCE).count(),
        2,
        "verify step + finish"
    );
    let layout = plain(&design_agent_system_prompt_for(
        DesignVerifyProtocol::LayoutSnapshot,
    ));
    assert!(
        !layout.contains(CADENCE),
        "layout hosts have no screenshots"
    );
    assert!(layout.contains("snapshot_layout"));
}

#[test]
fn the_font_check_names_fontissues_and_the_brief() {
    for text in [
        plain(&skill_text("design-craft")),
        plain(SCREENSHOT_VERIFY_STEP),
    ] {
        assert!(text.contains("fontIssues"), "{text}");
        assert!(text.contains("family must match the brief"), "{text}");
    }
}

#[test]
fn the_wrapper_comes_before_the_sections() {
    for text in [
        skill_text("design-craft"),
        SCREENSHOT_VERIFY_STEP.to_string(),
    ] {
        assert!(text.contains("wrapper frame first, then the sections"));
    }
}

#[test]
fn the_generic_default_check_sits_in_direction_without_repeating_avoid() {
    let craft = skill_text("design-craft");
    let direction = craft
        .lines()
        .find(|line| line.starts_with("DIRECTION."))
        .expect("DIRECTION paragraph");
    assert!(direction.contains("is this the generic default for this kind of product?"));
    assert!(direction.contains("Name one decision that only fits THIS product"));
    // The AVOID list is still written once, not copied into the check.
    assert_eq!(craft.matches("AVOID by default").count(), 1);
    for item in ["purple-to-pink", "gradient text", "eyebrow label"] {
        assert_eq!(
            craft.matches(item).count(),
            1,
            "{item} must stay in AVOID only"
        );
    }
}

#[test]
fn the_brief_asks_for_a_content_inventory_before_building() {
    let brief = skill_text("design-brief");
    let inventory = brief
        .find("Content inventory per screen")
        .expect("inventory bullet");
    assert!(brief[inventory..].contains("so every size comes from content"));
    // It is part of the brief written before the first batch.
    assert!(inventory < brief.find("ASSUMPTIONS").expect("assumptions"));
    assert!(inventory > brief.find("3. WRITE THE BRIEF").expect("brief step"));
    assert!(!brief.contains("Step 3"));
}

#[test]
fn craft_tells_the_model_to_re_plan_after_two_failed_fixes() {
    assert!(skill_text("design-craft")
        .contains("If one issue survives two fixes, stop patching: re-plan that part."));
}

#[test]
fn the_summary_rule_replaced_an_older_sentence_and_the_file_did_not_grow() {
    let file = crate::phase_corpus_dir()
        .and_then(|dir| dir.get_file("phases/agent/design-agent.md"))
        .expect("design-agent.md is embedded");
    let text = file.contents_utf8().expect("utf-8");
    assert!(
        text.len() <= DESIGN_AGENT_BYTES_BEFORE_PACK_A,
        "design-agent.md is {} bytes; Pack A may not add to it",
        text.len()
    );
    assert!(text.contains(
        "End with Done (what you built, root node ids), Remaining (unfinished), Unsure (guessed choices): one line each."
    ));
    assert!(!text.contains("one-line summary of what you built"));
    // The no-dump rule that shared the sentence is kept.
    assert!(text.contains("Do not dump JSON, node trees, or raw DSL into the chat."));
    // And it reaches both hosts' prompts.
    for verify in [
        DesignVerifyProtocol::Screenshot,
        DesignVerifyProtocol::LayoutSnapshot,
    ] {
        assert!(design_agent_system_prompt_for(verify).contains("Remaining (unfinished)"));
    }
}

#[test]
fn pack_a_stays_inside_the_skill_budgets() {
    let craft = estimate_tokens(&skill_text("design-craft"));
    let brief = estimate_tokens(&skill_text("design-brief"));
    let wcag = estimate_tokens(&skill_text("design-wcag-quickcheck"));
    assert!(craft <= 1_300, "design-craft {craft}");
    assert!(brief <= 1_100, "design-brief {brief}");
    let vector = estimate_tokens(&skill_text("design-vector-drawing"));
    assert!(vector <= 650, "design-vector-drawing {vector}");
    let all_four = craft + brief + wcag + vector;
    assert!(all_four <= 2_400, "{all_four}");
}
