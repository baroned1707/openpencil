use super::*;

const HINT: &str = "The user chose this style for this run: follow it unless they say \
otherwise; design.md and the pinned style (if any) outrank it.\nStyle: Glassmorphism.";

fn request(style_hint: Option<&str>) -> DesignRequest {
    DesignRequest {
        prompt: "Design a fintech dashboard".into(),
        style_hint: style_hint.map(str::to_string),
        ..Default::default()
    }
}

const MODES: [PlanningMode; 3] = [
    PlanningMode::Rich,
    PlanningMode::Minimal,
    PlanningMode::Compact,
];

#[test]
fn a_style_hint_joins_the_planning_user_prompt_in_every_mode() {
    for mode in MODES {
        let prompt = build_orchestrator_prompt(&request(Some(HINT)), mode, AbortFlag::new());
        let user = &prompt.call_request.user_prompt;
        assert!(user.contains(HINT), "{mode:?}");
        // After the brief, as its own paragraph.
        assert!(user.contains(&format!("\n\n{HINT}")), "{mode:?}");
        // The brief itself is untouched: design-type detection reads it.
        assert!(
            user.starts_with("Design a fintech dashboard")
                || user.contains("Design a fintech dashboard")
        );
        assert!(
            !prompt.call_request.system_prompt.contains(HINT),
            "{mode:?}"
        );
    }
}

#[test]
fn planning_prompts_are_byte_identical_without_a_style_hint() {
    for mode in MODES {
        let none = build_orchestrator_prompt(&request(None), mode, AbortFlag::new());
        let empty = build_orchestrator_prompt(&request(Some("")), mode, AbortFlag::new());
        let with = build_orchestrator_prompt(&request(Some(HINT)), mode, AbortFlag::new());
        assert_eq!(
            none.call_request.user_prompt,
            empty.call_request.user_prompt
        );
        assert_eq!(
            none.call_request.system_prompt, with.call_request.system_prompt,
            "the hint never changes the system prompt"
        );
        assert_ne!(none.call_request.user_prompt, with.call_request.user_prompt);
    }
}

#[test]
fn a_request_without_a_style_hint_serialises_exactly_as_before() {
    // The retry stash stores the request as JSON: no `styleHint` key may appear
    // for a run that carried no choice.
    let json = serde_json::to_string(&request(None)).expect("json");
    assert!(!json.contains("styleHint"), "{json}");
    let with = serde_json::to_string(&request(Some(HINT))).expect("json");
    assert!(with.contains("styleHint"));
    let back: DesignRequest = serde_json::from_str(&with).expect("round trip");
    assert_eq!(back.style_hint.as_deref(), Some(HINT));
    let old: DesignRequest = serde_json::from_str(r#"{"prompt":"x"}"#).expect("old json");
    assert_eq!(old.style_hint, None);
}
