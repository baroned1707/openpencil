//! The tools an agent leans on (render, layout, read, write with its
//! diagnostics, finalize, export) run on a PARKED document through the host's
//! lens while another document stays on screen, and none of them leaves a
//! trace on the document the user is looking at.
//!
//! Real HTTP against the real live server (`Fixture`): `a` and `b` are parked,
//! `c` is on screen. Before each call the on-screen document is made
//! non-default (selection, camera, an undoable edit, a second chat state) so
//! "unchanged" cannot pass on default == default.

use super::multi_document_e2e_support::*;
use base64::Engine;
use op_editor_core::{DocumentId, NodeId};
use serde_json::{json, Value};

/// Everything the user can see or lose of the document on screen.
#[derive(Debug, PartialEq)]
struct OnScreenTrace {
    selection: String,
    viewport: String,
    active_page: usize,
    revision: u64,
    undo_depth: usize,
    redo_depth: usize,
    document_json: String,
    epoch: u64,
    tab_order: Vec<DocumentId>,
    active_tab: DocumentId,
    dirty: bool,
}

impl Fixture {
    /// Give the on-screen document a non-default look: a selection, a moved
    /// camera and one undoable edit.
    fn make_on_screen_document_non_default(&mut self) {
        let state = self.app.host.editor_state_mut();
        state.set_single_selection(NodeId::new(FRAME_ID));
        state.commit_history();
        assert!(state.set_selected_color(true, "#336699"));
        state.mark_document_changed();
        state.viewport.pan_x = 37.0;
        state.viewport.pan_y = -12.0;
        state.viewport.zoom = 1.5;
        assert!(state.history.can_undo(), "the edit is undoable");
    }

    fn on_screen_trace(&mut self) -> OnScreenTrace {
        self.app.sync_active_slot();
        let state = self.app.host.editor_state();
        let trace = OnScreenTrace {
            selection: format!("{:?}", state.selection),
            viewport: format!("{:?}", state.viewport),
            active_page: state.ui.active_page_index,
            revision: state.document_revision(),
            undo_depth: state.history.past.len(),
            redo_depth: state.history.future.len(),
            document_json: serde_json::to_string(&state.doc).expect("doc json"),
            epoch: self.app.host.document_epoch(),
            tab_order: (0..self.app.documents.len())
                .map(|index| self.app.documents.id_at(index).expect("tab"))
                .collect(),
            active_tab: self.app.documents.active_id(),
            dirty: self.app.document_is_dirty(),
        };
        trace
    }

    /// Call `tool` on the parked document `id` over the generic `/mcp` route.
    fn call_on(&mut self, id: DocumentId, tool: &str, mut arguments: Value) -> Reply {
        arguments["documentId"] = json!(Fixture::name(id));
        self.call("/mcp", tool, arguments)
    }
}

/// The base64 payload of the first image block of a tool reply.
fn image_block_bytes(reply: &Reply) -> Vec<u8> {
    let blocks = reply.body["result"]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("no content: {:?}", reply.body));
    let data = blocks
        .iter()
        .find(|block| block["type"] == "image")
        .and_then(|block| block["data"].as_str())
        .unwrap_or_else(|| panic!("no image block: {:?}", reply.body));
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64 image")
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A card with one visible, filled rectangle, so a render has pixels to show.
const CARD_SCRIPT: &str = r##"
I(null, {"type":"frame","id":"lens-card","name":"Lens Card","x":400,"y":0,
  "width":240,"height":160,"layout":"none",
  "fill":[{"type":"solid","color":"#FFCC00"}],
  "children":[
    {"type":"frame","id":"lens-chip","name":"Chip","x":20,"y":20,"width":80,"height":40,
     "fill":[{"type":"solid","color":"#CC0033"}]}
  ]});
"##;

/// A card that clips an absolutely placed button (`clipped-control`).
const DEFECTIVE_SCRIPT: &str = r##"
I(null, {"type":"frame","id":"bad-card","name":"Card","x":700,"y":0,"width":294,"height":236,
  "layout":"none","clipContent":true,"fill":[{"type":"solid","color":"#FFFFFF"}],
  "children":[
    {"type":"frame","id":"bad-add","role":"button","x":282,"y":176,"width":44,"height":44}
  ]});
"##;

/// The id the editor gave the node named `name` (a script's own ids are
/// replaced by fresh ones).
fn node_named(fx: &mut Fixture, id: DocumentId, name: &str) -> Option<String> {
    fn walk(node: &Value, name: &str) -> Option<String> {
        if node["name"] == name {
            return node["id"].as_str().map(str::to_string);
        }
        node["children"]
            .as_array()?
            .iter()
            .find_map(|child| walk(child, name))
    }
    let doc = fx.peek(id, |state| {
        serde_json::to_value(&state.doc).expect("doc json")
    });
    doc["children"]
        .as_array()?
        .iter()
        .find_map(|child| walk(child, name))
}

/// A fixture whose parked document `a` holds the card (its editor-assigned
/// `(card, chip)` ids come with it), with `c` made non-default on top. The
/// card is written through the lens; the tests that follow check what their
/// own call did to the screen.
fn fixture_with_card(tag: &str) -> (Fixture, String, String) {
    let mut fx = Fixture::new(tag);
    let reply = fx.call_on(fx.a, "batch_design", json!({"script": CARD_SCRIPT}));
    assert!(!reply.is_error(), "{}", reply.text());
    let a = fx.a;
    let card = node_named(&mut fx, a, "Lens Card").expect("card landed");
    let chip = node_named(&mut fx, a, "Chip").expect("chip landed");
    fx.make_on_screen_document_non_default();
    (fx, card, chip)
}

fn assert_on_screen_untouched(fx: &mut Fixture, before: &OnScreenTrace, what: &str) {
    let after = fx.on_screen_trace();
    assert_eq!(&after, before, "{what} changed the document on screen");
}

#[test]
fn the_default_catalog_serves_every_tool_the_lens_tests_use() {
    let mut fx = Fixture::new("lens-catalog");
    let mut list = fx.drive(vec![Box::new(move |port| {
        post_raw(
            port,
            "/mcp",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        )
    }) as Job<Result<Reply, String>>]);
    let list = list.remove(0).expect("HTTP exchange");
    let names: Vec<String> = list.body["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    for tool in [
        "get_screenshot",
        "snapshot_layout",
        "read_nodes",
        "batch_design",
        "finalize_design",
        "export_nodes",
    ] {
        assert!(names.iter().any(|name| name == tool), "{tool} missing");
    }
}

#[test]
fn get_screenshot_renders_a_parked_document_as_a_real_png() {
    let (mut fx, card, _chip) = fixture_with_card("lens-shot");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(fx.a, "get_screenshot", json!({"nodeId": card, "scale": 1}));

    assert!(!reply.is_error(), "{}", reply.text());
    let png = image_block_bytes(&reply);
    assert_eq!(png[..8], PNG_SIGNATURE, "a PNG");
    assert!(
        png.len() > 200,
        "a PNG with pixels in it, got {} bytes",
        png.len()
    );
    assert_on_screen_untouched(&mut fx, &before, "get_screenshot");
}

#[test]
fn snapshot_layout_measures_a_parked_document() {
    let (mut fx, card, chip) = fixture_with_card("lens-layout");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(fx.a, "snapshot_layout", json!({}));

    assert!(!reply.is_error(), "{}", reply.text());
    let text = reply.text();
    assert!(text.contains(&card) && text.contains(&chip), "{text}");
    assert!(text.contains("\"width\":240"), "measured size: {text}");
    assert_on_screen_untouched(&mut fx, &before, "snapshot_layout");
}

#[test]
fn read_nodes_reads_a_parked_document() {
    let (mut fx, _card, chip) = fixture_with_card("lens-read");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(fx.a, "read_nodes", json!({"nodeIds": [chip]}));

    assert!(!reply.is_error(), "{}", reply.text());
    assert!(reply.text().contains(&chip), "{}", reply.text());
    assert!(reply.text().contains("Chip"), "{}", reply.text());
    assert_on_screen_untouched(&mut fx, &before, "read_nodes");
}

#[test]
fn batch_design_reports_post_write_diagnostics_for_a_parked_document() {
    let (mut fx, _card, _chip) = fixture_with_card("lens-diagnostics");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(fx.a, "batch_design", json!({"script": DEFECTIVE_SCRIPT}));

    assert!(!reply.is_error(), "{}", reply.text());
    let text = reply.text();
    assert!(
        text.contains("clipped-control") || text.contains("layoutIssues"),
        "the write carried its layout diagnostics: {text}"
    );
    let a = fx.a;
    assert!(
        node_named(&mut fx, a, "Card").is_some(),
        "the write landed in the parked document"
    );
    assert_on_screen_untouched(&mut fx, &before, "batch_design");
    assert!(!fx.fingerprint(fx.c).contains("\"Card\""));
}

#[test]
fn finalize_design_runs_on_a_parked_document() {
    let (mut fx, _card, _chip) = fixture_with_card("lens-finalize");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(fx.a, "finalize_design", json!({}));

    assert!(!reply.is_error(), "{}", reply.text());
    assert_on_screen_untouched(&mut fx, &before, "finalize_design");
}

#[test]
fn export_nodes_exports_a_parked_document() {
    let (mut fx, card, _chip) = fixture_with_card("lens-export");
    let before = fx.on_screen_trace();

    let reply = fx.call_on(
        fx.a,
        "export_nodes",
        json!({"nodeIds": [card], "format": "png"}),
    );

    assert!(!reply.is_error(), "{}", reply.text());
    let files = reply.json();
    let entry = &files["files"][0];
    assert!(entry.get("error").is_none(), "{entry}");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(entry["bytes_base64"].as_str().expect("bytes"))
        .expect("base64");
    assert_eq!(bytes[..8], PNG_SIGNATURE);
    assert_on_screen_untouched(&mut fx, &before, "export_nodes");
}

#[test]
fn two_parked_documents_are_served_in_turn_without_touching_the_screen() {
    let (mut fx, card, _chip) = fixture_with_card("lens-two-parked");
    let before = fx.on_screen_trace();

    // `a` holds the card, `b` still its starter frame.
    for (id, node) in [(fx.a, card), (fx.b, FRAME_ID.to_string())] {
        let layout = fx.call_on(id, "snapshot_layout", json!({}));
        assert!(!layout.is_error(), "{}", layout.text());
        assert!(layout.text().contains(&node), "{}", layout.text());
        let shot = fx.call_on(id, "get_screenshot", json!({"nodeId": node}));
        assert!(!shot.is_error(), "{}", shot.text());
        assert_eq!(image_block_bytes(&shot)[..8], PNG_SIGNATURE);
    }

    assert_on_screen_untouched(&mut fx, &before, "two parked reads");
}
