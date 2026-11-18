use std::sync::atomic::{AtomicUsize, Ordering};

use op_editor_core::EditorState;

use super::*;

/// The real in-app tool surface over a fixed document, counting calls.
struct InApp {
    state: Mutex<EditorState>,
    calls: AtomicUsize,
}

impl InApp {
    fn new(state: EditorState) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            calls: AtomicUsize::new(0),
        })
    }
}

impl ChatToolExecutor for InApp {
    fn execute(&self, name: &str, args_json: &str) -> ChatToolResult {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        crate::design_agent_tools::execute_agent_tool(&mut state, name, args_json).0
    }
}

/// An executor that replays fixed results in order.
struct Replay(Mutex<Vec<String>>);

impl ChatToolExecutor for Replay {
    fn execute(&self, _name: &str, _args_json: &str) -> ChatToolResult {
        ChatToolResult {
            content: self.0.lock().unwrap().remove(0),
            is_error: false,
        }
    }
}

fn guide_text(result: &ChatToolResult) -> String {
    let envelope: Value = serde_json::from_str(&result.content).expect("envelope json");
    envelope["data"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("no data.content in {}", result.content))
        .to_string()
}

#[test]
fn guideline_already_in_prompt_returns_a_reference() {
    let prompt =
        op_ai_skills::design_agent_system_prompt_with_skills("design a mobile app login screen");
    for component in op_ai_skills::guideline_components("mobile") {
        assert!(
            prompt.contains(&component),
            "precondition: the prompt-matched mobile skill is in the prompt"
        );
    }
    let inner = InApp::new(EditorState::new());
    let shaped = DesignRunExecutor::wrap(inner.clone(), &prompt);

    // The topic and its alias are both already in the instructions.
    for topic in ["mobile", "mobile-app"] {
        let result = shaped.execute("get_guidelines", &format!(r#"{{"topic":"{topic}"}}"#));
        assert!(!result.is_error, "{}", result.content);
        assert_eq!(
            guide_text(&result),
            format!("This guideline is already in your instructions: {topic}")
        );
        assert!(result.content.len() < 300, "{}", result.content);
    }
    // The host still ran the call (its transcript card completes normally).
    assert_eq!(inner.calls.load(Ordering::SeqCst), 2);

    // A topic the prompt does not hold in full is served in full.
    let dashboard = shaped.execute("get_guidelines", r#"{"topic":"dashboard"}"#);
    let components = op_ai_skills::guideline_components("dashboard");
    assert!(!components.iter().all(|c| prompt.contains(c.as_str())));
    assert!(guide_text(&dashboard).len() > 2000);
    // The style category is never short-circuited.
    let style = shaped.execute(
        "get_guidelines",
        r#"{"category":"style","name":"x","colorPalette":"x","roundness":"x","elevation":"x","headings":"x","body":"x","captions":"x","data":"x"}"#,
    );
    assert!(!style.content.contains("already in your instructions"));
    // An unknown topic is not "covered" either.
    let unknown = shaped.execute("get_guidelines", r#"{"topic":"nope"}"#);
    assert!(!unknown.content.contains("already in your instructions"));
}

fn write_result(layout: &[&str]) -> String {
    json!({ "success": true, "data": {}, "layoutIssues": layout, "layoutHint": "fix" }).to_string()
}

#[test]
fn repeated_write_feedback_is_counted_not_resent() {
    let shaped = DesignRunExecutor::wrap(
        Arc::new(Replay(Mutex::new(vec![
            write_result(&["a collapsed", "b overflow"]),
            write_result(&["a collapsed", "b overflow", "c clipped"]),
            write_result(&["c clipped"]),
            write_result(&["a collapsed"]),
        ]))),
        "",
    );
    let parsed =
        |result: &ChatToolResult| -> Value { serde_json::from_str(&result.content).unwrap() };
    let layout = |value: &Value| -> Vec<String> {
        value["layoutIssues"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    // First write: nothing earlier to repeat, byte-identical.
    let first = shaped.execute("batch_design", "{}");
    assert_eq!(first.content, write_result(&["a collapsed", "b overflow"]));
    // Second: two lines repeat, one is new.
    let second = parsed(&shaped.execute("batch_design", "{}"));
    assert_eq!(layout(&second), ["c clipped"]);
    assert_eq!(second["layoutIssuesUnchanged"], 2);
    // Third: only the one carried over from the second write repeats, and the
    // list empties out.
    let third = parsed(&shaped.execute("batch_design", "{}"));
    assert!(third.get("layoutIssues").is_none());
    assert_eq!(third["layoutIssuesUnchanged"], 1);
    // Fourth: "a collapsed" was fixed in the third write, so it is new again.
    let fourth = parsed(&shaped.execute("batch_design", "{}"));
    assert_eq!(layout(&fourth), ["a collapsed"]);
    assert!(fourth.get("layoutIssuesUnchanged").is_none());
}

#[test]
fn repeated_structured_contrast_issues_are_deduplicated_too() {
    let issue = |id: &str| json!({"nodeId": id, "fg": "#777777", "bg": "#888888", "ratio": 1.1, "target": 4.5});
    let result = |ids: &[&str]| {
        json!({"success": true, "contrastIssues": ids.iter().map(|id| issue(id)).collect::<Vec<_>>()})
            .to_string()
    };
    let shaped = DesignRunExecutor::wrap(
        Arc::new(Replay(Mutex::new(vec![
            result(&["t1", "t2"]),
            result(&["t1", "t3"]),
        ]))),
        "",
    );
    shaped.execute("batch_design", "{}");
    let second: Value =
        serde_json::from_str(&shaped.execute("batch_design", "{}").content).unwrap();
    assert_eq!(second["contrastIssues"], json!([issue("t3")]));
    assert_eq!(second["contrastIssuesUnchanged"], 1);
}

#[test]
fn the_completion_gates_pass_through_untouched() {
    struct Gated;
    impl ChatToolExecutor for Gated {
        fn execute(&self, _: &str, _: &str) -> ChatToolResult {
            unreachable!()
        }
        fn check_blockers(&self) -> BlockerReport {
            BlockerReport {
                blockers: vec![op_ai::chat_provider::BlockerEntry {
                    category: "nav".into(),
                    detail: "tab".into(),
                }],
            }
        }
    }
    let shaped = DesignRunExecutor::wrap(Arc::new(Gated), "");
    assert_eq!(shaped.check_blockers().blockers.len(), 1);
}

/// A dark root with `count` fill-less texts (each a contrast issue) and one
/// empty named shell, so the write feedback has more lines than the in-app cap
/// AND the document has a real completion blocker.
fn crowded_state(count: usize) -> EditorState {
    let mut children: Vec<Value> = (0..count)
        .map(|i| {
            json!({ "type": "text", "id": format!("text{i}"), "content": "Label",
                         "width": 80, "height": 20 })
        })
        .collect();
    children.push(
        json!({ "type": "frame", "id": "shell", "name": "RecentActivity",
                          "width": "fill_container", "height": "fit_content", "children": [] }),
    );
    let doc: jian_ops_schema::PenDocument = serde_json::from_value(json!({
        "version": "1.0",
        "children": [{ "type": "frame", "id": "root", "name": "Home", "width": 400, "height": 900,
                       "layout": "none", "fill": [{ "type": "solid", "color": "#0F172A" }],
                       "children": children }]
    }))
    .expect("document");
    EditorState::from_document(doc)
}

#[test]
fn in_app_feedback_is_capped_without_starving_the_blocker_ledger() {
    use crate::design_agent_tools::{
        post_write_diagnostics, PostWriteExtras, IN_APP_FEEDBACK_CAPS,
    };
    let state = crowded_state(60);

    let feedback = post_write_diagnostics(
        &state,
        PostWriteExtras::default(),
        Some(&IN_APP_FEEDBACK_CAPS),
    );
    let contrast = feedback["contrastIssues"]
        .as_array()
        .expect("contrastIssues");
    assert_eq!(contrast.len(), IN_APP_FEEDBACK_CAPS.max_items);
    // The loop keeps the structured objects it repairs from.
    assert!(contrast[0]["nodeId"].is_string() && contrast[0]["fg"].is_string());
    assert_eq!(feedback["diagnosticsTruncated"], json!(true));
    let size = Value::Object(feedback.clone()).to_string().len();
    assert!(size <= IN_APP_FEEDBACK_CAPS.max_total_bytes, "{size}");
    // The keys the loop repairs from survive the cap.
    assert!(feedback.contains_key("shellsRemaining"));

    // The same document, uncapped, carries all 60 — the cap is only what the
    // model is shown.
    let uncapped = post_write_diagnostics(&state, PostWriteExtras::default(), None);
    assert_eq!(uncapped["contrastIssues"].as_array().unwrap().len(), 60);

    // The completion gate recomputes from the live document, not from the
    // feedback: the empty shell is a blocker whatever was shown.
    let report = crate::loop_blocker_ledger::detect_blockers(&state);
    assert!(
        report
            .blockers
            .iter()
            .any(|b| b.category == "empty-shell" && b.detail.contains("RecentActivity")),
        "{report:?}"
    );

    // And the in-app wrapper really applies the cap end to end.
    let mut live = crowded_state(60);
    let args = r#"{"operations":"I(root, {\"type\":\"frame\",\"name\":\"Extra\",\"width\":50,\"height\":50})"}"#;
    let (result, _) =
        crate::design_agent_tools::execute_agent_tool(&mut live, "batch_design", args);
    let envelope: Value = serde_json::from_str(&result.content).expect("envelope");
    let items = envelope["contrastIssues"]
        .as_array()
        .unwrap_or_else(|| panic!("write result carries contrastIssues: {}", result.content));
    assert_eq!(items.len(), IN_APP_FEEDBACK_CAPS.max_items);
    assert!(items[0]["nodeId"].is_string());
}

#[test]
fn design_data_lookup_passes_through_the_loop_executor_untouched() {
    // category=data is not a guide topic, so the "already in your
    // instructions" shortcut must never swallow it; a JSON-number limit
    // (as models send it) must reach the tool as "2".
    let prompt =
        op_ai_skills::design_agent_system_prompt_with_skills("design a mobile app login screen");
    let inner = InApp::new(EditorState::new());
    let shaped = DesignRunExecutor::wrap(inner.clone(), &prompt);
    let result = shaped.execute(
        "get_guidelines",
        r#"{"category":"data","query":"fintech dashboard saas","limit":2}"#,
    );
    assert!(!result.is_error, "{}", result.content);
    let envelope: Value = serde_json::from_str(&result.content).expect("envelope json");
    let matches = envelope["data"]["matches"]
        .as_array()
        .unwrap_or_else(|| panic!("no data.matches in {}", result.content));
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0]["source"]["licenseSpdx"], "MIT");
    assert!(
        result.content.len() <= 6 * 1024 + 200,
        "{}",
        result.content.len()
    );
    assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn design_style_lookup_passes_through_the_loop_executor_untouched() {
    // category=data with kind=style is not a guide topic either: the loop
    // executor hands it to the same tool, once, and returns its JSON.
    let prompt =
        op_ai_skills::design_agent_system_prompt_with_skills("design a mobile app login screen");
    let inner = InApp::new(EditorState::new());
    let shaped = DesignRunExecutor::wrap(inner.clone(), &prompt);
    let result = shaped.execute(
        "get_guidelines",
        r#"{"category":"data","kind":"style","query":"glassmorphism"}"#,
    );
    assert!(!result.is_error, "{}", result.content);
    let envelope: Value = serde_json::from_str(&result.content).expect("envelope json");
    assert_eq!(envelope["data"]["matches"][0]["ref"], "glassmorphism");
    assert_eq!(
        envelope["data"]["matches"][0]["source"]["licenseSpdx"],
        "MIT"
    );
    assert!(
        result.content.len() <= 6 * 1024 + 200,
        "{}",
        result.content.len()
    );
    assert_eq!(inner.calls.load(Ordering::SeqCst), 1);

    let named = shaped.execute(
        "get_guidelines",
        r#"{"category":"data","kind":"style","query":"external:minimalism-and-swiss-style"}"#,
    );
    let named: Value = serde_json::from_str(&named.content).expect("envelope json");
    assert_eq!(
        named["data"]["matches"][0]["ref"],
        "minimalism-and-swiss-style"
    );
}
