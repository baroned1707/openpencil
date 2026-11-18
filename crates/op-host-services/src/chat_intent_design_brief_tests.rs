//! The `design-brief` skill is for NEW products: its trigger words must stay
//! disjoint from what the router reads as a modify request, so the two
//! vocabularies cannot drift into each other.

use super::*;

#[test]
fn design_brief_keywords_are_never_modify_requests() {
    let skill = op_ai_skills::get_skill_by_name("design-brief").expect("design-brief registered");
    let op_ai_skills::SkillTrigger::Keywords(keywords) = &skill.meta.trigger else {
        panic!("design-brief must be keyword-triggered");
    };
    assert!(!keywords.is_empty());
    for keyword in keywords {
        assert_ne!(
            classify_by_keywords(keyword),
            DesignIntent::Modify,
            "design-brief keyword {keyword:?} reads as a modify request"
        );
    }
    // A whole new-product sentence built from the keywords is not a modify.
    for prompt in [
        "design an app for tracking houseplants",
        "create a website for a ceramics studio",
        "build a booking app from scratch",
    ] {
        assert_ne!(
            classify_by_keywords(prompt),
            DesignIntent::Modify,
            "{prompt}"
        );
    }
}
