//! `finalize_design` tool tests — weak-model fixtures, no network.

use std::collections::BTreeMap;

use jian_ops_schema::node::PenNode;
use op_editor_core::{EditorCommand, EditorState};
use op_mcp::{McpTool, ToolErrorCode, ToolOutcome};

use super::finalize_tool::finalize_design_snapshot;

/// A weak-model screen: a childless rectangle whose only fill is an image
/// fill with a still-empty url — the shape `cleanup::materialize_empty_
/// image_fill_slots` converts into a real `PenNode::Image`.
const FIXTURE: &str = r##"{
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
        {
          "type": "rectangle",
          "id": "photo",
          "name": "Album Cover",
          "width": 240,
          "height": 160,
          "cornerRadius": 12,
          "fill": [{ "type": "image", "url": "" }]
        }
      ]
    }
  ]
}"##;

pub(super) fn load() -> EditorState {
    crate::doc_io::load_editor_state_from_source(FIXTURE, op_editor_core::Locale::EnUs)
        .expect("load finalize fixture")
}

pub(super) fn find<'a>(nodes: &'a [PenNode], id: &str) -> Option<&'a PenNode> {
    op_editor_core::walkers::find_node(nodes, &op_editor_core::NodeId::new(id.to_string()))
}

fn repairs_of(json: &str) -> u64 {
    let value: serde_json::Value = serde_json::from_str(json).expect("summary json");
    value["repairs"].as_u64().expect("repairs is a number")
}

#[test]
fn finalize_materializes_empty_image_fill_slot_and_is_idempotent() {
    let mut live = load();
    let tool = finalize_design_snapshot(&live);

    let outcome = tool.call(&BTreeMap::new());
    let (first_json, first_commands) = match outcome {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    assert!(!first_commands.is_empty(), "cleanup must record repairs");
    let first_repairs = repairs_of(&first_json);
    assert!(
        first_repairs > 0,
        "summary must report repairs: {first_json}"
    );

    // The host applier path: apply the recorded batch to the live state.
    assert!(
        live.apply(EditorCommand::Batch {
            commands: first_commands
        }),
        "the recorded cleanup batch must apply cleanly"
    );
    assert!(
        matches!(
            find(live.active_children(), "photo"),
            Some(PenNode::Image(_))
        ),
        "the empty image-fill rectangle must be materialized into an image node"
    );

    // Second call over the already-finalized document must be a true no-op:
    // no command-bearing outcome means the MCP host cannot bump the daemon's
    // document version for an unchanged tree.
    let revision_before_second = live.document_revision();
    let tool = finalize_design_snapshot(&live);
    let outcome = tool.call(&BTreeMap::new());
    let second_json = match outcome {
        ToolOutcome::OkJson(json) => json,
        other => panic!("second finalize must be plain OkJson, got {other:?}"),
    };
    let second_repairs = repairs_of(&second_json);
    assert_eq!(
        second_repairs, 0,
        "second finalize must report zero repairs: {second_json}"
    );
    assert_eq!(
        live.document_revision(),
        revision_before_second,
        "a no-op second finalize must not advance the live document revision"
    );
    assert!(
        matches!(
            find(live.active_children(), "photo"),
            Some(PenNode::Image(_))
        ),
        "the materialized image node must survive the second call"
    );
}

#[test]
fn finalize_accepts_root_ids_as_json_array_and_comma_list() {
    let live = load();
    let tool = finalize_design_snapshot(&live);

    for raw in [r#"["root"]"#, "root"] {
        let mut args = BTreeMap::new();
        args.insert("root_ids".to_string(), raw.to_string());
        let outcome = tool.call(&args);
        let json = match outcome {
            ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { .. }) => json,
            ToolOutcome::OkJson(json) => json,
            other => panic!("root_ids {raw:?} must finalize, got {other:?}"),
        };
        assert!(
            repairs_of(&json) > 0,
            "root_ids {raw:?} must repair the fixture: {json}"
        );
    }
}

fn json_of(outcome: ToolOutcome) -> serde_json::Value {
    match outcome {
        ToolOutcome::OkJson(json) | ToolOutcome::OkJsonWithCommand(json, _) => {
            serde_json::from_str(&json).expect("finalize result json")
        }
        other => panic!("expected a finalize result, got {other:?}"),
    }
}

fn finalize_with(tool: &super::finalize_tool::FinalizeDesignTool, root_ids: &str) -> ToolOutcome {
    let mut args = BTreeMap::new();
    args.insert("root_ids".to_string(), root_ids.to_string());
    tool.call(&args)
}

/// Invariant: a subset of the top-level frames finalizes just those frames and
/// says so; the complete set (duplicates collapsed) is the whole page and its
/// result carries no scope keys; an id that is not on the page is rejected.
#[test]
fn finalize_scopes_to_a_root_subset_and_reports_which_roots() {
    let live = load_fixture(
        r##"{
          "version":"1.1","children":[
            {"type":"frame","id":"left","name":"Left","width":600,"height":800,
             "layout":"vertical","children":[]},
            {"type":"frame","id":"right","name":"Right","width":600,"height":800,
             "layout":"vertical","children":[]}
          ]
        }"##,
    );
    let tool = finalize_design_snapshot(&live);

    let subset = json_of(finalize_with(&tool, r#"["left"]"#));
    assert_eq!(subset["scope"], "sections", "{subset}");
    assert_eq!(subset["finalizedRoots"], serde_json::json!(["left"]));
    assert_eq!(subset["roots"], 1);

    let whole = json_of(finalize_with(&tool, r#"["right","left","left"]"#));
    assert!(whole.get("scope").is_none() && whole.get("finalizedRoots").is_none());
    assert_eq!(whole["roots"], 2);

    match finalize_with(&tool, r#"["left","unknown"]"#) {
        ToolOutcome::Err(ToolErrorCode::InvalidArgument, message) => {
            assert!(message.contains("unknown"), "{message}")
        }
        other => panic!("an unknown id must be rejected, got {other:?}"),
    }
}

const PAGE: &str = r##"{
  "version":"1.1","children":[
    {"type":"frame","id":"page","name":"Page","width":1200,"height":900,
     "layout":"vertical","children":[
       {"type":"frame","id":"nav","name":"Nav","width":"fill_container",
        "layout":"horizontal","children":[
          {"type":"text","id":"nav-t","content":"Home","fontSize":15}]},
       {"type":"frame","id":"hero","name":"Hero","width":"fill_container",
        "layout":"vertical","children":[
          {"type":"text","id":"hero-t","content":"Hello","fontSize":40}]}
     ]}
  ]
}"##;

fn command_targets(outcome: &ToolOutcome) -> Vec<String> {
    let ToolOutcome::OkJsonWithCommand(_, EditorCommand::Batch { commands }) = outcome else {
        return Vec::new();
    };
    commands
        .iter()
        .filter_map(|command| match command {
            EditorCommand::PatchNodeData { node_id, .. } => Some(node_id.as_str().to_string()),
            _ => None,
        })
        .collect()
}

/// Invariant: `root_ids` naming a node INSIDE a section finalizes that
/// section and nothing else: the section gets its semantic repairs (here a
/// fill-less text gets a fill), a sibling section keeps its own fill-less
/// text, and the page wrapper gets no gap.
#[test]
fn finalize_scopes_to_the_section_that_contains_the_named_id() {
    let live = load_fixture(PAGE);
    let tool = finalize_design_snapshot(&live);

    let outcome = finalize_with(&tool, r#"["hero-t"]"#);
    let json = json_of(match &outcome {
        ToolOutcome::OkJson(j) | ToolOutcome::OkJsonWithCommand(j, _) => {
            ToolOutcome::OkJson(j.clone())
        }
        other => panic!("{other:?}"),
    });
    assert_eq!(
        json["finalizedRoots"],
        serde_json::json!(["hero"]),
        "{json}"
    );
    let targets = command_targets(&outcome);
    assert!(targets.contains(&"hero-t".to_string()), "{targets:?}");
    for untouched in ["nav", "nav-t", "page"] {
        assert!(
            !targets.contains(&untouched.to_string()),
            "{untouched} is outside the named section: {targets:?}"
        );
    }

    let whole = finalize_with(&tool, "");
    let whole_targets = command_targets(&whole);
    assert!(
        whole_targets.contains(&"nav-t".to_string()) || whole_targets.contains(&"page".to_string()),
        "the unscoped finalize repairs the other sections too: {whole_targets:?}"
    );
}

#[test]
fn finalize_summary_carries_checkpoints_records_and_credential() {
    let live = load();
    let tool = finalize_design_snapshot(&live);
    let outcome = tool.call(&BTreeMap::new());
    let json = match outcome {
        ToolOutcome::OkJsonWithCommand(json, _) => json,
        other => panic!("expected a batch outcome, got {other:?}"),
    };
    let value: serde_json::Value = serde_json::from_str(&json).expect("summary json");
    assert!(
        value["checkedCategories"]
            .as_array()
            .is_some_and(|c| !c.is_empty()),
        "checked checkpoint categories must be present: {json}"
    );
    assert!(
        value["repairRecords"]
            .as_array()
            .is_some_and(|r| !r.is_empty()),
        "itemized repair records must be present: {json}"
    );
    assert!(
        value["summary"]
            .as_str()
            .is_some_and(|s| !s.trim().is_empty()),
        "human-readable credential must be present: {json}"
    );
}

#[test]
fn finalize_tool_replays_app_semantics_promotion_and_state_hoist() {
    let mut live = load_fixture(
        r##"{
          "version":"1.1","formatVersion":"1.1","children":[{
            "type":"frame","id":"root","name":"Login","width":390,"height":844,
            "layout":"vertical","children":[{
              "type":"frame","id":"email","name":"Email input","role":"input",
              "width":342,"height":48,"layout":"horizontal",
              "state":{"email":{"type":"string","default":""}},
              "bindings":{"value":"$app.email"},
              "children":[
                {"type":"icon_font","id":"mail","iconFontName":"mail","width":20,"height":20},
                {"type":"text","id":"hint","content":"name@example.com",
                 "fill":[{"type":"solid","color":"#9CA3AF"}]}
              ]
            }]
          }]
        }"##,
    );
    let tool = finalize_design_snapshot(&live);
    let outcome = tool.call(&BTreeMap::new());
    let (json, commands) = match outcome {
        ToolOutcome::OkJsonWithCommand(json, EditorCommand::Batch { commands }) => (json, commands),
        other => panic!("whole App finalizer must return a replay batch, got {other:?}"),
    };
    assert!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap()["checkedCategories"]
            .as_array()
            .is_some_and(|categories| !categories.is_empty()),
        "full finalizer carries its deterministic quality credential: {json}"
    );
    assert!(live.apply(EditorCommand::Batch { commands }));
    assert!(matches!(
        find(live.active_children(), "email"),
        Some(PenNode::TextInput(_))
    ));
    assert!(
        find(live.active_children(), "hint").is_none(),
        "promotion consumes old visual children"
    );
    let document = serde_json::to_value(&live.doc).unwrap();
    assert_eq!(document["state"]["email"]["default"], "");
}

#[test]
fn finalize_tool_replays_same_id_section_action_leaf_conversion() {
    let mut live = load_fixture(
        r##"{
          "version":"1.1","formatVersion":"1.1","children":[{
            "type":"frame","id":"root","name":"Shop Home","width":390,"height":844,
            "layout":"vertical","children":[{
              "type":"frame","id":"section-header","name":"Featured Header",
              "width":"fill_container","height":40,"layout":"horizontal",
              "children":[
                {"type":"text","id":"heading","name":"Section title","content":"Featured",
                 "fontSize":20,"fontWeight":700},
                {"type":"text","id":"see-all","name":"See all","content":"View all >",
                 "fontSize":14}
              ]
            }]
          }]
        }"##,
    );

    let tool = finalize_design_snapshot(&live);
    let outcome = tool.call(&BTreeMap::new());
    let commands = match outcome {
        ToolOutcome::OkJsonWithCommand(_, EditorCommand::Batch { commands }) => commands,
        other => panic!("same-id semantic leaf rewrite must be replayable, got {other:?}"),
    };
    assert!(live.apply(EditorCommand::Batch { commands }));
    let Some(PenNode::IconFont(icon)) = find(live.active_children(), "see-all") else {
        panic!("section-header action should become an icon_font leaf");
    };
    assert_eq!(icon.icon_font_name, "chevron-right");
}

/// Shared loader for the inline JSON fixtures used by both this module
/// and the advisory sibling (`finalize_tool_advisory_tests`).
pub(super) fn load_fixture(source: &str) -> EditorState {
    crate::doc_io::load_editor_state_from_source(source, op_editor_core::Locale::EnUs)
        .expect("load drift fixture")
}

#[path = "finalize_tool_verdict_tests.rs"]
mod verdict_tests;
