//! A stock hit's credit lands on the node in the same apply as its url.

use super::super::*;
use super::*;

fn credit() -> op_image_enrich::ImageCredit {
    op_image_enrich::ImageCredit {
        provider: "openverse".into(),
        creator: Some("Jane Doe".into()),
        license: Some("CC BY 4.0".into()),
        license_url: Some("https://creativecommons.org/licenses/by/4.0/".into()),
        source_page_url: Some("https://openverse.org/image/abc".into()),
    }
}

fn state_with_slot() -> EditorState {
    let mut state = EditorState::new();
    state.active_children_mut().clear();
    state
        .active_children_mut()
        .push(image_node("photo", "", Some("ceramic vase")));
    state
}

fn credit_of(state: &EditorState) -> Option<op_image_enrich::ImageCredit> {
    walkers::find_node(state.active_children(), &NodeId::new("photo".to_string()))
        .and_then(|node| op_editor_core::node_image_credit(node).cloned())
}

#[test]
fn desktop_session_applies_image_and_credit_in_one_command() {
    let mut state = state_with_slot();
    let target = collect_targets(&state, &HashSet::new()).remove(0);
    let mut session = ImageSearchSession::new();
    // The memo holds url AND credit: a repeat query must land the same credit,
    // not only the same url (it would otherwise be cleared on apply).
    session.search_memo.lock().unwrap().insert(
        search_intent_key(
            &target.query,
            target.aspect_ratio,
            target.prompt.as_deref(),
            target.judge_view,
        ),
        SearchMemoEntry::Ready(FoundImage {
            url: "https://example.test/vase.jpg".into(),
            credit: Some(credit()),
        }),
    );

    let revision = state.document_revision();
    assert!(session.enqueue_missing(&state));
    assert!(session.poll_into(&mut state));

    let PenNode::Image(image) = &state.active_children()[0] else {
        panic!("image")
    };
    assert_eq!(image.src, "https://example.test/vase.jpg");
    assert_eq!(credit_of(&state), Some(credit()));
    // One apply: a single revision bump covers the url and the credit.
    assert!(state.document_revision() > revision);
}

#[test]
fn a_credit_less_job_result_clears_a_stale_credit() {
    let mut state = state_with_slot();
    op_editor_core::set_node_image_credit(&mut state.active_children_mut()[0], Some(credit()));
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(JobOutcome::Search(Some(
        "https://example.test/new.jpg".into(),
    )))
    .unwrap();
    let mut session = ImageSearchSession::new();
    let node_id = NodeId::new("photo".to_string());
    session.in_flight.insert("photo".to_string());
    session.jobs.push(ImageSearchJob {
        node_id,
        deadline: None,
        intent: None,
        stock_view: None,
        fallback_target: None,
        rx,
    });

    assert!(session.poll_into(&mut state));
    assert_eq!(credit_of(&state), None);
}
