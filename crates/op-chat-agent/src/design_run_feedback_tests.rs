//! The run-wide "report once" memory and the re-plan count, driven through
//! [`DesignRunExecutor`] the way the loop drives it.

use std::sync::{Arc, Mutex};

use op_ai::chat_provider::{ChatToolExecutor, ChatToolResult};
use serde_json::{json, Value};

use crate::design_run_executor::DesignRunExecutor;

/// Replays fixed results in order; a `None` entry is a rolled-back write
/// (an error result).
struct Script(Mutex<Vec<Option<String>>>);

impl ChatToolExecutor for Script {
    fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
        match self.0.lock().unwrap().remove(0) {
            Some(content) => ChatToolResult {
                content,
                is_error: false,
            },
            None => ChatToolResult {
                content: r#"{"success":false,"error":"Transaction rolled back"}"#.into(),
                is_error: true,
            },
        }
    }
}

fn layout(lines: &[&str]) -> Option<String> {
    Some(json!({ "success": true, "layoutIssues": lines }).to_string())
}

fn run(results: Vec<Option<String>>) -> Arc<dyn ChatToolExecutor> {
    DesignRunExecutor::wrap(Arc::new(Script(Mutex::new(results))), "")
}

fn write(shaped: &Arc<dyn ChatToolExecutor>, page: Option<&str>) -> Value {
    let args = page.map_or_else(
        || "{}".to_string(),
        |id| json!({ "pageId": id }).to_string(),
    );
    let result = shaped.execute("batch_design", &args);
    serde_json::from_str(&result.content).expect("json result")
}

#[test]
fn a_line_is_reported_once_even_after_a_write_to_another_page() {
    let shaped = run(vec![
        layout(&["a collapsed"]),
        Some(json!({ "success": true }).to_string()),
        layout(&["a collapsed"]),
    ]);
    assert_eq!(
        write(&shaped, Some("pageA"))["layoutIssues"][0],
        "a collapsed"
    );
    // Page B is clean, and says nothing about page A.
    write(&shaped, Some("pageB"));
    let back = write(&shaped, Some("pageA"));
    assert!(back.get("layoutIssues").is_none(), "{back}");
    assert_eq!(back["layoutIssuesUnchanged"], 1);
}

#[test]
fn a_fixed_issue_is_reported_again_when_it_returns() {
    let shaped = run(vec![
        layout(&["a collapsed"]),
        Some(json!({ "success": true }).to_string()),
        layout(&["a collapsed"]),
    ]);
    write(&shaped, None);
    // A clean write on the SAME page: the issue was fixed.
    write(&shaped, None);
    let back = write(&shaped, None);
    assert_eq!(back["layoutIssues"], json!(["a collapsed"]), "{back}");
    assert!(back.get("layoutIssuesUnchanged").is_none(), "{back}");
}

#[test]
fn a_write_cut_by_a_cap_does_not_make_a_hidden_line_look_fixed() {
    let cut = Some(
        json!({ "success": true, "layoutIssues": ["b clipped"], "diagnosticsTruncated": true })
            .to_string(),
    );
    let shaped = run(vec![
        layout(&["a collapsed"]),
        cut,
        layout(&["a collapsed"]),
    ]);
    write(&shaped, None);
    // "a collapsed" is missing here only because the cap dropped it.
    write(&shaped, None);
    let third = write(&shaped, None);
    assert!(third.get("layoutIssues").is_none(), "{third}");
    assert_eq!(third["layoutIssuesUnchanged"], 1);
}

#[test]
fn a_cap_that_dropped_a_whole_key_does_not_make_its_lines_look_fixed() {
    // The 8 KB cap can remove a list entirely: the key is absent but the
    // write says it cut something.
    let cut = Some(json!({ "success": true, "diagnosticsTruncated": true }).to_string());
    let shaped = run(vec![
        layout(&["a collapsed"]),
        cut,
        layout(&["a collapsed"]),
    ]);
    write(&shaped, None);
    write(&shaped, None);
    let third = write(&shaped, None);
    assert_eq!(third["layoutIssuesUnchanged"], 1, "{third}");
}

#[test]
fn the_third_identical_line_adds_one_replan_hint_and_only_then() {
    let line = "node \"Hero\" (n9): text overflows its frame";
    let shaped = run(vec![
        layout(&[line]),
        layout(&[line]),
        layout(&[line]),
        layout(&[line]),
    ]);
    for _ in 0..2 {
        let early = write(&shaped, None);
        assert!(early.get("replanHint").is_none(), "{early}");
    }
    let third = write(&shaped, None);
    let hint = third["replanHint"]
        .as_str()
        .expect("hint on the third write");
    assert!(hint.contains(line), "{hint}");
    assert!(hint.contains("new plan"), "{hint}");
    // The repeated line itself is still only counted.
    assert_eq!(third["layoutIssuesUnchanged"], 1);
    // One hint, not one per later write.
    assert!(write(&shaped, None).get("replanHint").is_none());
}

#[test]
fn the_count_restarts_when_the_line_goes_away_or_changes() {
    let shaped = run(vec![
        layout(&["a"]),
        layout(&["a"]),
        layout(&[]),
        layout(&["a"]),
        layout(&["a"]),
        layout(&["b"]),
        layout(&["a"]),
    ]);
    for _ in 0..7 {
        let result = write(&shaped, None);
        assert!(result.get("replanHint").is_none(), "{result}");
    }
}

#[test]
fn a_rolled_back_write_neither_counts_nor_resets_the_streak() {
    let shaped = run(vec![layout(&["a"]), None, layout(&["a"]), layout(&["a"])]);
    write(&shaped, None);
    let rolled_back = write(&shaped, None);
    assert_eq!(rolled_back["success"], false);
    assert!(write(&shaped, None).get("replanHint").is_none());
    assert!(write(&shaped, None).get("replanHint").is_some());
}

#[test]
fn a_contrast_issue_is_followed_by_its_node_across_changing_colours() {
    let result = |fg: &str| {
        Some(
            json!({ "success": true, "contrastIssues": [
                { "nodeId": "t7", "fg": fg, "bg": "#101010", "ratio": 2.0, "target": 4.5 }
            ] })
            .to_string(),
        )
    };
    let shaped = run(vec![
        result("#555555"),
        result("#666666"),
        result("#777777"),
    ]);
    write(&shaped, None);
    write(&shaped, None);
    let third = write(&shaped, None);
    // Different colours: every attempt is a new object, so none is dropped...
    assert!(third.get("contrastIssuesUnchanged").is_none(), "{third}");
    // ...but it is the same node three times, which is the failed-fix signal.
    assert!(
        third["replanHint"].as_str().unwrap().contains("t7"),
        "{third}"
    );
}

#[test]
fn open_by_design_keys_never_ask_for_a_re_plan() {
    let slots = || {
        Some(
            json!({ "success": true, "imageSlots": ["slot Hero (n3)"],
                     "contrastUncertain": [{ "nodeId": "t1" }] })
            .to_string(),
        )
    };
    let shaped = run(vec![slots(), slots(), slots(), slots()]);
    for _ in 0..4 {
        assert!(write(&shaped, None).get("replanHint").is_none());
    }
}

#[test]
fn several_lines_reaching_three_still_add_a_single_hint() {
    let shaped = run(vec![
        layout(&["a", "b"]),
        layout(&["a", "b"]),
        layout(&["a", "b"]),
    ]);
    write(&shaped, None);
    write(&shaped, None);
    let third = write(&shaped, None);
    let hint = third["replanHint"].as_str().expect("one hint");
    assert!(hint.contains("(and 1 more)"), "{hint}");
}

#[test]
fn a_clean_write_stays_byte_identical() {
    let clean = json!({ "success": true, "data": {} }).to_string();
    let shaped = run(vec![Some(clean.clone())]);
    let result = shaped.execute("batch_design", "{}");
    assert_eq!(result.content, clean);
}
