//! `enrich_images` tool tests — injected stub search backends, no network.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jian_ops_schema::node::PenNode;
use jian_ops_schema::style::PenFill;
use op_editor_core::{EditorCommand, EditorState, NodeId};
use op_image_enrich::{FoundImage, ImageCredit, ImageSearchTarget};
use op_mcp::{McpTool, ToolOutcome};

use super::enrich_images_tool::{
    run_enrich_sync, search_parallel_before, EnrichImagesTool, EnrichSummary, ImageSearchAttempt,
    ImageSearchBackend,
};

/// A scripted backend: `query -> url`, with a call log so tests can prove a
/// Generate prompt never reached a search.
#[derive(Default)]
struct StubBackend {
    urls: HashMap<String, String>,
    credits: HashMap<String, ImageCredit>,
    calls: Mutex<Vec<String>>,
}

impl StubBackend {
    fn with(query: &str, url: &str) -> Self {
        Self {
            urls: HashMap::from([(query.to_string(), url.to_string())]),
            ..Self::default()
        }
    }
}

impl StubBackend {
    fn with_credit(mut self, query: &str, credit: ImageCredit) -> Self {
        self.credits.insert(query.to_string(), credit);
        self
    }
}

impl ImageSearchBackend for StubBackend {
    fn search(&self, target: &ImageSearchTarget) -> Option<FoundImage> {
        self.calls.lock().unwrap().push(target.query.clone());
        let url = self.urls.get(&target.query).cloned()?;
        Some(FoundImage {
            url,
            credit: self.credits.get(&target.query).cloned(),
            download_ping: None,
        })
    }
}

/// A backend that sleeps before answering, to drive the deadline path.
/// Sleeps PAST whatever deadline the loop hands it before answering, so the
/// first target deterministically lands late and every later target is
/// deterministically gated to NotStarted — no dependence on a fixed sleep
/// racing the real clock.
struct SleepingBackend {
    url: String,
}

struct ParallelProbeBackend {
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl ImageSearchBackend for ParallelProbeBackend {
    fn search(&self, _target: &ImageSearchTarget) -> Option<FoundImage> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Some("data:image/jpeg;base64,AQ==".into())
    }

    fn search_many_before(
        &self,
        targets: &[ImageSearchTarget],
        deadline: Instant,
    ) -> Vec<ImageSearchAttempt> {
        search_parallel_before(self, targets, deadline)
    }
}

impl ImageSearchBackend for SleepingBackend {
    fn search(&self, _target: &ImageSearchTarget) -> Option<FoundImage> {
        Some(self.url.as_str().into())
    }

    fn search_before(&self, _target: &ImageSearchTarget, deadline: Instant) -> Option<FoundImage> {
        // Overshoot the deadline by a margin regardless of the budget size.
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(150),
        );
        Some(self.url.as_str().into())
    }
}

/// One Search-bound empty image node + one Generate-bound image node.
const MIXED_FIXTURE: &str = r##"{
  "version": "1.0",
  "children": [
    {
      "type": "frame",
      "id": "root",
      "name": "Landing",
      "width": 390,
      "height": 844,
      "layout": "vertical",
      "children": [
        { "type": "image", "id": "photo", "name": "Hero", "src": "", "imageSearchQuery": "mountain lake", "width": 320, "height": 180 },
        { "type": "image", "id": "art", "name": "Art", "src": "", "imagePrompt": "paint a moonlit forest", "width": 320, "height": 180 }
      ]
    }
  ]
}"##;

/// An empty image-fill slot on a Frame (the 0814 shape).
const EMPTY_FILL_FIXTURE: &str = r##"{
  "version": "1.0",
  "children": [
    {
      "type": "frame",
      "id": "slot",
      "name": "Banner",
      "width": 320,
      "height": 180,
      "imageSearchQuery": "forest trail",
      "fill": [{ "type": "image", "url": "" }]
    }
  ]
}"##;

fn load(source: &str) -> EditorState {
    crate::doc_io::load_editor_state_from_source(source, op_editor_core::Locale::EnUs)
        .expect("load enrich fixture")
}

fn find<'a>(nodes: &'a [PenNode], id: &str) -> Option<&'a PenNode> {
    op_editor_core::walkers::find_node(nodes, &NodeId::new(id.to_string()))
}

fn summary_of(json: &str) -> EnrichSummary {
    let value: serde_json::Value = serde_json::from_str(json).expect("enrich summary json");
    EnrichSummary {
        targets: value["targets"].as_u64().unwrap_or_default() as usize,
        resolved: value["resolved"].as_u64().unwrap_or_default() as usize,
        failed: value["failed"].as_u64().unwrap_or_default() as usize,
        unresolved: value["unresolved"].as_u64().unwrap_or_default() as usize,
    }
}

#[test]
fn enrich_fills_search_slots_and_fails_generate_without_searching() {
    let live = load(MIXED_FIXTURE);
    let backend = Arc::new(StubBackend::with(
        "mountain lake",
        "data:image/png;base64,AA==",
    ));
    let backend_dyn: Arc<dyn ImageSearchBackend> = backend.clone();
    let tool = EnrichImagesTool::for_test(&live, backend_dyn);

    let mut args = BTreeMap::new();
    args.insert("timeout_seconds".to_string(), "30".to_string());
    let outcome = tool.call(&args);
    let (json, commands) = match outcome {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    assert_eq!(
        summary_of(&json),
        EnrichSummary {
            targets: 2,
            resolved: 1,
            failed: 1,
            unresolved: 0,
        }
    );

    // The Generate slot must never have been offered to the search backend.
    let calls: Vec<String> = backend.calls.lock().unwrap().clone();
    assert_eq!(calls, ["mountain lake".to_string()], "search-only contract");

    // The host applier path: apply the recorded writes.
    let mut live = live;
    assert!(live.apply(EditorCommand::Batch { commands }));

    let photo = find(live.active_children(), "photo").expect("photo survives");
    let PenNode::Image(photo) = photo else {
        panic!("photo must stay an image node");
    };
    assert_eq!(photo.src, "data:image/png;base64,AA==");

    let art = find(live.active_children(), "art").expect("art survives");
    let PenNode::Frame(art) = art else {
        panic!("failed art must become a fallback frame");
    };
    assert!(
        art.base
            .name
            .as_deref()
            .is_some_and(|name| name.ends_with(" (image fallback)")),
        "explicit Generate must fail visibly"
    );
    assert!(art
        .base
        .explain
        .as_deref()
        .is_some_and(|explain| explain.starts_with("image fallback:")));
}

#[test]
fn enrich_lands_url_on_empty_image_fill_slot() {
    let live = load(EMPTY_FILL_FIXTURE);
    let backend = Arc::new(StubBackend::with(
        "forest trail",
        "data:image/jpeg;base64,AQ==",
    ));
    let tool = EnrichImagesTool::for_test(&live, backend);

    let outcome = tool.call(&BTreeMap::new());
    let (json, commands) = match outcome {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    assert_eq!(
        summary_of(&json),
        EnrichSummary {
            targets: 1,
            resolved: 1,
            failed: 0,
            unresolved: 0,
        }
    );

    let mut live = live;
    assert!(live.apply(EditorCommand::Batch { commands }));

    let slot = find(live.active_children(), "slot").expect("slot survives");
    let PenNode::Frame(frame) = slot else {
        panic!("slot must stay a frame");
    };
    let Some([PenFill::Image(body)]) = frame.container.fill.as_deref() else {
        panic!("slot must carry exactly one image fill");
    };
    assert_eq!(body.url, "data:image/jpeg;base64,AQ==");
}

#[test]
fn enrich_timeout_leaves_unstarted_targets_unresolved() {
    // Two targets, a backend that always finishes after the deadline it is
    // given: the first target's search starts before the deadline and its
    // late result still lands; the second is never started and counts
    // unresolved. The budget clock starts at the search phase, so local
    // target collection can never starve the run before the first search.
    let source = r##"{
      "version": "1.0",
      "children": [
        { "type": "image", "id": "one", "src": "", "imageSearchQuery": "lake one", "width": 120, "height": 80 },
        { "type": "image", "id": "two", "src": "", "imageSearchQuery": "lake two", "width": 120, "height": 80 }
      ]
    }"##;
    let live = load(source);
    let backend = Arc::new(SleepingBackend {
        url: "data:image/jpeg;base64,AQ==".to_string(),
    });
    let tool = EnrichImagesTool::for_test(&live, backend);

    let mut args = BTreeMap::new();
    args.insert("timeout_seconds".to_string(), "1".to_string());
    let outcome = tool.call(&args);
    let json = match outcome {
        ToolOutcome::OkJsonWithCommand(json, _) => json,
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    assert_eq!(
        summary_of(&json),
        EnrichSummary {
            targets: 2,
            resolved: 1,
            failed: 0,
            unresolved: 1,
        }
    );
}

#[test]
fn production_parallel_search_is_bounded_to_three_workers() {
    let source = serde_json::json!({
        "version": "1.0",
        "children": (0..6).map(|index| serde_json::json!({
            "type": "image",
            "id": format!("image-{index}"),
            "src": "",
            "imageSearchQuery": format!("product {index}"),
            "width": 120,
            "height": 80,
        })).collect::<Vec<_>>(),
    })
    .to_string();
    let mut state = load(&source);
    let backend = ParallelProbeBackend {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
    };
    let run = run_enrich_sync(&mut state, None, Duration::from_secs(2), &backend);
    assert_eq!(run.summary.resolved, 6);
    assert_eq!(backend.peak.load(Ordering::SeqCst), 3);
}

#[test]
fn enrich_expired_deadline_marks_every_target_unresolved() {
    // Drive the core loop directly with a zero budget: the search-phase
    // deadline is already due, so no search may start and the Search slot
    // counts unresolved. The Generate slot is a local write-back the budget
    // never gates, so it still lands its failed-search sentinel.
    let mut state = load(MIXED_FIXTURE);
    let backend = StubBackend::default();
    let run = run_enrich_sync(&mut state, None, Duration::ZERO, &backend);
    assert_eq!(
        run.summary,
        EnrichSummary {
            targets: 2,
            resolved: 0,
            failed: 1,
            unresolved: 1,
        }
    );
    assert!(
        backend.calls.lock().unwrap().is_empty(),
        "an expired deadline must not start any search"
    );
}

#[test]
fn enrich_timeout_parsing_rejects_and_clamps() {
    let live = load(EMPTY_FILL_FIXTURE);
    let backend = Arc::new(StubBackend::default());
    let tool = EnrichImagesTool::for_test(&live, backend);

    for (raw, expected_err) in [("0", true), ("soon", true), ("-1", true)] {
        let mut args = BTreeMap::new();
        args.insert("timeout_seconds".to_string(), raw.to_string());
        match tool.call(&args) {
            ToolOutcome::Err(op_mcp::ToolErrorCode::InvalidArgument, _) => {
                assert!(expected_err, "timeout_seconds {raw:?} must reject");
            }
            other => panic!("timeout_seconds {raw:?} must reject, got {other:?}"),
        }
    }
    // Above the cap clamps to MAX_TIMEOUT_SECONDS (600) instead of rejecting.
    let mut args = BTreeMap::new();
    args.insert("timeout_seconds".to_string(), "9999".to_string());
    let outcome = tool.call(&args);
    assert!(
        matches!(
            outcome,
            ToolOutcome::OkJsonWithCommand(_, _) | ToolOutcome::OkJson(_)
        ),
        "an over-cap timeout must clamp and succeed"
    );
}

#[test]
fn enrich_rejects_unknown_root_ids() {
    let live = load(EMPTY_FILL_FIXTURE);
    let backend = Arc::new(StubBackend::default());
    let tool = EnrichImagesTool::for_test(&live, backend);

    let mut args = BTreeMap::new();
    args.insert("root_ids".to_string(), "nope".to_string());
    match tool.call(&args) {
        ToolOutcome::Err(op_mcp::ToolErrorCode::InvalidArgument, message) => {
            assert!(
                message.contains("nope"),
                "message must name the bad id: {message}"
            );
        }
        other => panic!("unknown root must reject, got {other:?}"),
    }
}

#[test]
fn enrich_scopes_to_requested_roots() {
    // Two roots; only the one named in root_ids holds the target.
    let source = r##"{
      "version": "1.0",
      "children": [
        { "type": "frame", "id": "a", "name": "A", "width": 200, "height": 200,
          "children": [{ "type": "image", "id": "img_a", "src": "", "imageSearchQuery": "lake a", "width": 120, "height": 80 }] },
        { "type": "frame", "id": "b", "name": "B", "width": 200, "height": 200,
          "children": [{ "type": "image", "id": "img_b", "src": "", "imageSearchQuery": "lake b", "width": 120, "height": 80 }] }
      ]
    }"##;
    let live = load(source);
    let backend = Arc::new(StubBackend::with("lake b", "data:image/png;base64,AQ=="));
    let tool = EnrichImagesTool::for_test(&live, backend);

    let mut args = BTreeMap::new();
    args.insert("root_ids".to_string(), "b".to_string());
    let outcome = tool.call(&args);
    let (json, commands) = match outcome {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    assert_eq!(
        summary_of(&json),
        EnrichSummary {
            targets: 1,
            resolved: 1,
            failed: 0,
            unresolved: 0,
        }
    );

    let mut live = live;
    assert!(live.apply(EditorCommand::Batch { commands }));
    let img_b = find(live.active_children(), "img_b").expect("img_b survives");
    let PenNode::Image(img_b) = img_b else {
        panic!("img_b must stay an image node");
    };
    assert_eq!(img_b.src, "data:image/png;base64,AQ==");
    let img_a = find(live.active_children(), "img_a").expect("img_a survives");
    let PenNode::Image(img_a) = img_a else {
        panic!("img_a must stay an image node");
    };
    assert_eq!(img_a.src, "", "the out-of-scope slot must stay untouched");
}

fn stock_credit() -> ImageCredit {
    ImageCredit {
        provider: "wikimedia".into(),
        creator: Some("Jane Doe".into()),
        license: Some("CC BY 4.0".into()),
        license_url: Some("https://creativecommons.org/licenses/by/4.0/".into()),
        source_page_url: Some("https://commons.wikimedia.org/wiki/File:Lake.jpg".into()),
    }
}

fn credit_of(state: &EditorState, id: &str) -> Option<ImageCredit> {
    find(state.active_children(), id)
        .and_then(|node| op_editor_core::node_image_credit(node).cloned())
}

fn run_batch(live: &EditorState, backend: StubBackend) -> Vec<EditorCommand> {
    let tool = EnrichImagesTool::for_test(live, Arc::new(backend));
    match tool.call(&BTreeMap::new()) {
        ToolOutcome::OkJsonWithCommand(_, EditorCommand::Batch { commands }) => commands,
        other => panic!("expected a batch outcome, got {other:?}"),
    }
}

#[test]
fn enrich_images_command_carries_the_credit_and_applies_to_a_live_state() {
    // An image node and a frame whose image FILL receives the picture.
    for (fixture, query, id) in [
        (MIXED_FIXTURE, "mountain lake", "photo"),
        (EMPTY_FILL_FIXTURE, "forest trail", "slot"),
    ] {
        let live = load(fixture);
        let backend = StubBackend::with(query, "data:image/png;base64,AA==")
            .with_credit(query, stock_credit());
        let commands = run_batch(&live, backend);

        // The credit is IN the command, next to the url, not applied beside it.
        let patches: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                EditorCommand::PatchNodeData {
                    node_id,
                    patch_json,
                    ..
                } if node_id.as_str() == id => Some(patch_json.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(patches.len(), 1, "{id}: one patch for the slot");
        let patch: serde_json::Value = serde_json::from_str(patches[0]).unwrap();
        assert_eq!(patch["imageCredit"]["creator"], "Jane Doe", "{id}");
        assert_eq!(patch["imageCredit"]["provider"], "wikimedia", "{id}");

        // Applied through the pump's unit (one Batch): url and credit land
        // together, and one undo removes both.
        let mut live = live;
        assert!(live.apply(EditorCommand::Batch { commands }));
        assert_eq!(credit_of(&live, id), Some(stock_credit()), "{id}");
        assert!(live.undo(), "{id}: the batch is one undo step");
        assert_eq!(credit_of(&live, id), None, "{id}");
    }
}

#[test]
fn enrich_images_without_a_credit_clears_a_stale_one_on_the_live_state() {
    let source = EMPTY_FILL_FIXTURE.replace(
        r#""imageSearchQuery": "forest trail","#,
        r#""imageSearchQuery": "forest trail", "imageCredit": {"provider": "openverse", "creator": "Old Creator"},"#,
    );
    let live = load(&source);
    assert!(
        credit_of(&live, "slot").is_some(),
        "fixture carries a credit"
    );
    let backend = StubBackend::with("forest trail", "data:image/jpeg;base64,AQ==");
    let commands = run_batch(&live, backend);

    // The removal is explicit (`null`): a shallow merge would keep the old key.
    let patch = commands
        .iter()
        .find_map(|command| match command {
            EditorCommand::PatchNodeData { patch_json, .. } => Some(patch_json.clone()),
            _ => None,
        })
        .expect("patch");
    let patch: serde_json::Value = serde_json::from_str(&patch).unwrap();
    assert!(patch["imageCredit"].is_null(), "{patch}");

    let mut live = live;
    assert!(live.apply(EditorCommand::Batch { commands }));
    assert_eq!(credit_of(&live, "slot"), None);
}
