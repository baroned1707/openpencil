//! `with_parked_document`: run code against a parked document without the
//! user seeing anything happen to the one on screen.

use super::*;
use op_editor_core::NodeId;

fn state_named(frame_id: &str) -> op_editor_core::EditorState {
    let src = format!(
        r##"{{"version":"{}","children":[{{"type":"frame","id":"{frame_id}","name":"{frame_id}",
            "x":0,"y":0,"width":400,"height":300,"children":[]}}]}}"##,
        env!("CARGO_PKG_VERSION")
    );
    let doc = jian_ops_schema::load_str(&src)
        .expect("fixture parses")
        .value;
    op_editor_core::EditorState::from_document(doc)
}

fn host_showing(frame_id: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    assert!(host.swap_document(&mut ParkedDocument::from_state(
        state_named(frame_id),
        Default::default()
    )));
    host
}

/// Everything about the on-screen document the lens must give back.
fn on_screen_fingerprint(host: &WidgetHostNative) -> String {
    format!(
        "{:?}|epoch={}|node_id={}|owner={}|dirty={}|chat_owner={}|chat_idx={}",
        host.editor_state(),
        host.document_epoch,
        host.next_node_id,
        host.layer_panel_owner,
        host.editor_state_dirty,
        host.chat_panel_owner,
        host.last_chat_session_index,
    )
}

fn parked_name(parked: &ParkedDocument, id: &str) -> Option<String> {
    use op_editor_core::PenNodeExt;
    op_editor_core::walkers::find_node(&parked.scope().doc().children, &NodeId::new(id))
        .and_then(|node| node.base().name.clone())
}

#[test]
fn lens_restores_the_on_screen_document_bit_for_bit() {
    let mut host = host_showing("docA");
    host.editor_state_mut().selection.set = vec![NodeId::new("docA")];
    host.editor_state_mut().viewport.pan(12.0, 34.0);
    host.editor_state_mut()
        .chat
        .input
        .set_text("half-typed prompt");
    let before = on_screen_fingerprint(&host);
    let epoch_before = host.document_epoch();

    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    let epoch_b = parked_b.epoch();
    let seen_inside = host.with_parked_document(&mut parked_b, |host| {
        // Inside the lens the host IS document B.
        let epoch = host.document_epoch();
        let state = host.editor_state_mut();
        assert!(state.rename_node(&NodeId::new("docB"), "renamed by a run"));
        state
            .chat
            .messages
            .push(op_editor_core::ChatMessage::user("B only"));
        host.mark_editor_state_dirty();
        epoch
    });

    assert_eq!(seen_inside, Some(epoch_b));
    assert_eq!(on_screen_fingerprint(&host), before);
    assert_eq!(host.document_epoch(), epoch_before);
    assert_eq!(parked_b.epoch(), epoch_b);
    assert_eq!(
        parked_name(&parked_b, "docB").as_deref(),
        Some("renamed by a run"),
        "the run's edit landed in the parked document"
    );
    assert_eq!(parked_b.scope().chat().messages.len(), 1);
    assert!(
        host.editor_state().chat.messages.is_empty(),
        "and its transcript did not reach the document on screen"
    );
}

#[test]
fn lens_has_no_user_visible_side_effects() {
    let mut host = host_showing("docA");
    // A rename draft that nobody committed yet.
    assert!(host
        .editor_state_mut()
        .start_rename_layer(NodeId::new("docA")));
    assert!(host.editor_state_mut().rename_append("X"));
    host.option_drag_source_ids.push(NodeId::new("docA"));
    host.chat_panel_owner = 41;
    host.last_chat_session_index = 3;
    let revision = host.editor_state().document_revision();

    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    assert!(host
        .with_parked_document(&mut parked_b, |_host| ())
        .is_some());

    assert!(
        host.editor_state().ui.layer_rename.is_some(),
        "the lens must not commit pending input"
    );
    assert_eq!(host.editor_state().document_revision(), revision);
    assert_eq!(
        host.option_drag_source_ids,
        vec![NodeId::new("docA")],
        "gestures are left running"
    );
    assert_eq!(host.chat_panel_owner, 41, "the chat owner is not rotated");
    assert_eq!(host.last_chat_session_index, 3);
}

#[test]
fn lens_is_not_refused_by_a_session_on_the_screen_document() {
    // The refusal depends on the TARGET document. A session bound to the one
    // on screen says nothing about a plain parked document, which stays
    // reachable (parked MCP calls, agent runs and recovery keep working).
    let mut host = host_showing("docA");
    host.editor_state_mut().editor_ui.collab.phase = op_editor_core::CollabConnectionPhase::Active;
    let before = on_screen_fingerprint(&host);
    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    let mut ran = false;

    let result = host.with_parked_document(&mut parked_b, |_host| ran = true);

    assert!(result.is_some());
    assert!(ran);
    assert_eq!(on_screen_fingerprint(&host), before);
}

#[test]
fn lens_refuses_a_parked_document_that_has_a_session_bound() {
    let mut host = host_showing("docA");
    let before = on_screen_fingerprint(&host);
    let mut collaborated = state_named("docB");
    collaborated.editor_ui.collab.phase = op_editor_core::CollabConnectionPhase::Active;
    let mut parked_b = ParkedDocument::from_state(collaborated, Default::default());
    let mut ran = false;

    let result = host.with_parked_document(&mut parked_b, |_host| ran = true);

    assert!(result.is_none());
    assert!(!ran, "the closure must not run against a shared document");
    assert_eq!(on_screen_fingerprint(&host), before);
    assert!(
        host.editor_state().editor_ui.editor_toast.is_none(),
        "a routine refusal raises no toast"
    );
}
