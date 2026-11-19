//! The eval harness end to end: the committed fixtures against the committed
//! baseline, determinism, and the entry-point rules.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::eval_gate::{Baseline, Thresholds};
use super::*;

fn fixture_from(id: &str, dir: &Path, case: Value) -> Fixture {
    Fixture {
        id: id.to_string(),
        dir: dir.to_path_buf(),
        file: serde_json::from_value(case).expect("case.json shape"),
    }
}

/// A scratch folder unique to this process and test, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("op-smoke-eval-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const ONE_FRAME: &str = "I(null, {type:'frame', name:'Card', width:300, height:200, layout:'vertical', gap:8, padding:16, fill:'#FFFFFF', children: []});";

fn script_case(script_text: &str, steps_extra: Vec<Value>, expect: Value) -> Value {
    let mut steps = vec![json!({ "tool": "batch_design", "args": { "script": script_text } })];
    steps.extend(steps_extra);
    json!({ "prompt": "design a card", "steps": steps, "expect": expect })
}

/// Invariant: every committed fixture, replayed now, matches the committed
/// baseline and thresholds. `OPENPENCIL_SMOKE_EVAL_BLESS=1` (outside CI)
/// rewrites the baseline instead.
#[test]
fn design_eval_matches_baseline() {
    let outcome = run(&default_dir(), &EvalEnv::from_process()).expect("eval runs");
    match outcome {
        Outcome::Blessed(count) => println!("baseline rewritten for {count} case(s)"),
        Outcome::Gated(report) => {
            println!("{}", report.table());
            assert!(
                report.passed(),
                "regressions:\n{}",
                report.failures.join("\n")
            );
        }
    }
}

/// Invariant: replaying a fixture twice gives identical document bytes and
/// identical metrics (stable ids, no clock, no randomness).
#[test]
fn replay_twice_is_byte_identical() {
    let fixtures = load_fixtures(&default_dir().join("cases")).expect("fixtures");
    assert!(
        fixtures.len() >= 10,
        "ten starter fixtures, found {}",
        fixtures.len()
    );
    for fixture in fixtures {
        let first = eval_replay::replay(&fixture).expect("first replay");
        let second = eval_replay::replay(&fixture).expect("second replay");
        assert_eq!(
            first.doc_json, second.doc_json,
            "{}: document bytes differ",
            fixture.id
        );
        assert_eq!(
            first.metrics, second.metrics,
            "{}: metrics differ",
            fixture.id
        );
        assert!(
            first.problems.is_empty(),
            "{}: {:?}",
            fixture.id,
            first.problems
        );
    }
}

/// Invariant: the Lorem-ipsum negative case is scored with placeholder text,
/// and the gate FAILS on it (it is not a baseline case).
#[test]
fn negative_lorem_ipsum_case_fails_the_gate() {
    let dir = default_dir();
    assert!(
        !load_fixtures(&dir.join("cases"))
            .unwrap()
            .iter()
            .any(|f| f.id.contains("lorem")),
        "the negative case must not live under cases/"
    );
    let negatives = load_fixtures(&dir.join("negative")).expect("negative fixtures");
    let fixture = negatives
        .iter()
        .find(|f| f.id == "lorem-ipsum")
        .expect("lorem-ipsum case");
    let scored = score_case(fixture).expect("score");
    assert_eq!(
        scored.metrics["placeholderText"], 3,
        "Title, Lorem ipsum, Description"
    );

    // Even a baseline that already held the placeholders cannot excuse them.
    let thresholds: Thresholds = read_json(&dir.join("thresholds.json")).unwrap();
    let now = BTreeMap::from([(fixture.id.clone(), scored.clone())]);
    let baseline = Baseline { cases: now.clone() };
    let report = eval_gate::gate(&now, &baseline, &thresholds);
    assert!(!report.passed(), "{}", report.table());
    assert!(
        report
            .failures
            .iter()
            .any(|line| line.contains("placeholderText")),
        "{:?}",
        report.failures
    );
}

/// Invariant: `get_screenshot` is counted and skipped, a failing step is
/// counted as a replay error, and batches are counted.
#[test]
fn replay_counts_screenshots_batches_and_failed_steps() {
    let scratch = Scratch::new("counts");
    let case = script_case(
        ONE_FRAME,
        vec![
            json!({ "tool": "get_screenshot", "args": { "nodeId": "root" } }),
            json!({ "tool": "get_screenshot", "args": { "nodeId": "root" } }),
            json!({ "tool": "no_such_tool", "args": {} }),
        ],
        json!({}),
    );
    let replayed = eval_replay::replay(&fixture_from("counts", &scratch.0, case)).unwrap();
    let get = |name: &str| replayed.metrics[name];
    assert_eq!(
        (
            get("batchCount"),
            get("screenshotCount"),
            get("replayErrors")
        ),
        (1, 2, 1)
    );
    assert!(
        replayed.problems.iter().any(|p| p.contains("no_such_tool")),
        "{:?}",
        replayed.problems
    );
}

/// Invariant: expectations (required text, min / max bounds) count as
/// `expectationFailures`, and a step with both or neither of `args` and
/// `scriptFile` is a setup error.
#[test]
fn expectations_are_counted_and_bad_steps_are_errors() {
    let scratch = Scratch::new("expect");
    let case = script_case(
        ONE_FRAME,
        vec![],
        json!({ "requiredText": ["Sign up"], "min": { "screenCount": 2 }, "max": { "nodeCount": 1000 } }),
    );
    let replayed = eval_replay::replay(&fixture_from("expect", &scratch.0, case)).unwrap();
    assert_eq!(replayed.metrics["requiredTextMissing"], 1);
    assert_eq!(
        replayed.metrics["expectationFailures"], 1,
        "{:?}",
        replayed.problems
    );

    let bad = json!({ "prompt": "x", "steps": [{ "tool": "get_editor_state" }] });
    assert!(matches!(
        eval_replay::replay(&fixture_from("bad", &scratch.0, bad)),
        Err(EvalError::BadStep { index: 0, .. })
    ));
}

/// Invariant: only nodes OUTSIDE the scope root count, whether added, removed
/// or changed; edits inside the scope subtree are free.
#[test]
fn outside_scope_changes_ignore_the_scope_subtree() {
    let node = |id: &str, extra: Value, children: Vec<Value>| {
        let mut value = json!({ "type": "frame", "id": id, "children": children });
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };
    let doc = |hero_title: &str, footer_gap: u32, extra: Vec<Value>| {
        let mut children = vec![
            node(
                "hero",
                json!({}),
                vec![node("title", json!({ "name": hero_title }), vec![])],
            ),
            node("footer", json!({ "gap": footer_gap }), vec![]),
        ];
        children.extend(extra);
        json!({ "version": "1.0", "children": [node("page", json!({}), children)] })
    };
    let seed = doc("a", 4, vec![]);
    let count = |after: &Value| eval_replay::outside_scope_changes(&seed, after, "hero");
    assert_eq!(count(&doc("b", 4, vec![])), 0, "inside the hero is free");
    assert_eq!(count(&doc("a", 8, vec![])), 1, "footer changed");
    assert_eq!(
        count(&doc("a", 4, vec![node("extra", json!({}), vec![])])),
        1,
        "node added"
    );
}

/// Invariant: `finalizeOnlyChanges` counts nodes the scripted calls did not
/// touch that the loop-end repairs changed (inside the scope too), and counts
/// neither a node the script edited nor one it inserted.
#[test]
fn finalize_only_changes_skip_what_the_script_touched() {
    let doc = |hero_gap: u32, title: &str, footer_role: Option<&str>, extra: bool| {
        let mut footer = json!({ "type": "frame", "id": "footer", "children": [] });
        if let Some(role) = footer_role {
            footer["role"] = json!(role);
        }
        let mut hero_children = vec![json!({
            "type": "text", "id": "title", "content": title, "children": []
        })];
        if extra {
            hero_children.push(json!({ "type": "text", "id": "note", "content": "x" }));
        }
        let hero = json!({
            "type": "frame", "id": "hero", "gap": hero_gap, "children": hero_children
        });
        json!({ "version": "1.0", "children": [{
            "type": "frame", "id": "page", "children": [hero, footer]
        }] })
    };
    let seed = doc(4, "a", None, false);
    let changed = |edited: &Value, finalized: &Value| {
        eval_replay::finalize_only_changed_ids(&seed, edited, finalized)
    };
    assert!(changed(&seed, &seed).is_empty(), "nothing finalized");
    assert_eq!(
        changed(&seed, &doc(4, "a", Some("footer"), false)),
        ["footer"],
        "a node the script left alone, repaired outside the scope"
    );
    assert_eq!(
        changed(&seed, &doc(8, "a", None, false)),
        ["hero"],
        "a node the script left alone, repaired inside the scope"
    );
    let edited = doc(4, "b", None, true);
    assert!(
        changed(&edited, &doc(4, "b", None, true)).is_empty(),
        "the script's own edit and insert are free"
    );
    assert!(
        changed(&edited, &doc(9, "c", None, true)).contains(&"hero".to_string()),
        "finalize still counts a node it changed beside the edited ones"
    );
    assert!(
        !changed(&edited, &doc(4, "c", None, true)).contains(&"title".to_string()),
        "a node the script edited is never finalize-only"
    );
}

/// Invariant: regeneration needs the explicit switch and never happens in CI.
#[test]
fn bless_needs_the_explicit_switch_and_is_refused_in_ci() {
    let env = |bless, ci| EvalEnv { bless, ci };
    assert_eq!(env(false, false).bless_allowed(), Ok(false));
    assert_eq!(env(false, true).bless_allowed(), Ok(false));
    assert_eq!(env(true, false).bless_allowed(), Ok(true));
    assert_eq!(
        env(true, true).bless_allowed(),
        Err(EvalError::BlessRefusedInCi)
    );
}

/// Invariant: a run without the switch never writes the baseline; with the
/// switch it does; with the switch in CI it refuses before doing anything.
#[test]
fn run_writes_the_baseline_only_when_blessed() {
    let scratch = Scratch::new("bless");
    let cases = scratch.0.join("cases").join("card");
    std::fs::create_dir_all(&cases).unwrap();
    std::fs::copy(
        default_dir().join("thresholds.json"),
        scratch.0.join("thresholds.json"),
    )
    .unwrap();
    let write_case = |text: &str| {
        let script = format!(
            "const f = I(null,{{type:'frame',name:'Card',width:300,height:200,layout:'vertical',fill:'#FFFFFF'}}); I(f,{{type:'text',content:'{text}',fontSize:16,fill:'#111111'}});"
        );
        let case = script_case(&script, vec![], json!({}));
        std::fs::write(cases.join("case.json"), case.to_string()).unwrap();
    };
    let baseline_path = scratch.0.join("baseline.json");

    write_case("Pay now");
    let blessed = run(
        &scratch.0,
        &EvalEnv {
            bless: true,
            ci: false,
        },
    )
    .unwrap();
    assert!(matches!(blessed, Outcome::Blessed(1)));
    let written = std::fs::read(&baseline_path).unwrap();

    // Same case, no switch: passes and leaves the file alone.
    let Outcome::Gated(report) = run(&scratch.0, &EvalEnv::default()).unwrap() else {
        panic!("no switch must gate, not bless");
    };
    assert!(report.passed(), "{:?}", report.failures);

    // A regression without the switch fails and still leaves the file alone.
    write_case("Lorem ipsum");
    let Outcome::Gated(report) = run(&scratch.0, &EvalEnv::default()).unwrap() else {
        panic!("no switch must gate, not bless");
    };
    assert!(!report.passed());
    assert_eq!(std::fs::read(&baseline_path).unwrap(), written);

    // The switch in CI is refused and the file is still untouched.
    let refused = run(
        &scratch.0,
        &EvalEnv {
            bless: true,
            ci: true,
        },
    );
    assert!(matches!(refused, Err(EvalError::BlessRefusedInCi)));
    assert_eq!(std::fs::read(&baseline_path).unwrap(), written);
}

/// Invariant: every case's rendered design-agent prompt stays within the
/// token ceiling and loads at least one skill; the baseline stores skill
/// names and counts only, never prompt text.
#[test]
fn prompt_render_stays_within_ceiling() {
    let dir = default_dir();
    let thresholds: Thresholds = read_json(&dir.join("thresholds.json")).unwrap();
    for fixture in load_fixtures(&dir.join("cases")).unwrap() {
        let render = eval_prompt::render_prompt(&fixture.file.prompt);
        assert!(
            !render.skills.is_empty(),
            "{}: no skills loaded",
            fixture.id
        );
        assert!(
            render.token_estimate <= thresholds.prompt_token_ceiling,
            "{}: {} tokens over the ceiling {}",
            fixture.id,
            render.token_estimate,
            thresholds.prompt_token_ceiling
        );
    }
    let baseline = read_text(&dir.join("baseline.json")).unwrap();
    assert!(
        !baseline.contains("You are a product designer"),
        "no prompt text in the baseline"
    );
}
