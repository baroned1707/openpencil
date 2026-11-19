//! Image-slot enrichment of every open document: a parked document's slot is
//! filled without switching to it, a tab switch keeps its jobs, a closed
//! document drops its results, and two documents that reuse one node id never
//! receive each other's pictures. The network is a per-thread stub
//! (`image_search_session::parked::stub_searches`); no test reaches a provider.

use super::*;
use crate::image_search_session::parked::{
    release_held_searches, stub_searches, StubbedFinder, StubbedSearch,
};
use op_editor_core::{walkers, EditorState, NodeId};
use op_host_services::doc_io::save_to_path;
use op_image_enrich::{FoundImage, ImageCredit};
use std::path::PathBuf;
use std::time::Duration;

/// The slot's node id in EVERY test document: the collision the per-document
/// sessions exist to survive.
const SLOT: &str = "hero";

/// Restores the real ladder when a test ends, whatever happens in it.
struct StubGuard;

impl StubGuard {
    fn new(stub: StubbedSearch) -> Self {
        stub_searches(Some(stub));
        Self
    }
}

impl Drop for StubGuard {
    fn drop(&mut self) {
        stub_searches(None);
    }
}

fn url_for(query: &str) -> String {
    format!("https://images.example/{}.jpg", query.replace(' ', "-"))
}

fn credit_for(query: &str) -> ImageCredit {
    ImageCredit {
        provider: "openverse".into(),
        creator: Some(format!("Photographer of {query}")),
        license: Some("CC BY 4.0".into()),
        license_url: None,
        source_page_url: None,
    }
}

/// Every search finds `https://images.example/<query>.jpg`, credited.
fn find_by_query(query: &str) -> Option<FoundImage> {
    Some(FoundImage {
        url: url_for(query),
        credit: Some(credit_for(query)),
        download_ping: None,
    })
}

const FINDER: StubbedFinder = find_by_query;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-imgenrich-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A document whose only content is one empty image slot (`SLOT`) asking for
/// `query`.
fn slot_state(query: &str) -> EditorState {
    let mut state = EditorState::starter();
    let node: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "image", "id": SLOT, "name": "Hero photo", "src": "",
        "width": 240, "height": 160, "imageSearchQuery": query,
    }))
    .expect("image node");
    state.active_children_mut().clear();
    state.active_children_mut().push(node);
    state
}

/// Open one document per query (the last is on screen). Every parked document
/// is edited once, as an agent's write would, so its content counts as changed.
fn app_with_slot_documents(
    tag: &str,
    queries: &[&str],
) -> (DesktopApp, Vec<op_editor_core::DocumentId>, PathBuf) {
    let dir = temp_dir(tag);
    let mut app = DesktopApp::new(None);
    for (index, query) in queries.iter().enumerate() {
        let path = dir.join(format!("doc{index}.op"));
        save_to_path(&slot_state(query), &path).expect("fixture saves");
        assert!(app.open_document_tab(path));
    }
    let ids: Vec<_> = (0..queries.len())
        .map(|index| app.documents.id_at(index).expect("tab"))
        .collect();
    let on_screen = app.documents.active_index();
    for index in 0..queries.len() - 1 {
        assert!(app.activate_document_slot(index));
        app.host.editor_state_mut().mark_document_changed();
    }
    assert!(app.activate_document_slot(on_screen));
    (app, ids, dir)
}

/// What the slot holds in document `id`: `(src, credit)`.
fn slot_of(app: &mut DesktopApp, id: op_editor_core::DocumentId) -> (String, Option<ImageCredit>) {
    let read = |state: &EditorState| {
        let node =
            walkers::find_node(state.active_children(), &NodeId::new(SLOT)).expect("slot node");
        let jian_ops_schema::node::PenNode::Image(image) = node else {
            panic!("the slot is an image node");
        };
        (
            image.src.to_string(),
            op_editor_core::node_image_credit(node).cloned(),
        )
    };
    if id == app.documents.active_id() {
        return read(app.host.editor_state());
    }
    let index = app.documents.index_of(id).expect("open document");
    let mut parked = app.documents.take_parked(index).expect("parked");
    let result = app
        .host
        .with_parked_document(&mut parked, |host| read(host.editor_state()));
    app.documents.restore_parked(index, parked);
    result.expect("lens ran")
}

fn later(now: Instant, millis: u64) -> Instant {
    now + Duration::from_millis(millis)
}

#[test]
fn a_parked_documents_slot_is_filled_without_switching_to_it() {
    let _stub = StubGuard::new(StubbedSearch::Answer(FINDER));
    let (mut app, ids, dir) = app_with_slot_documents("fill", &["alpha", "beta"]);
    let (parked, on_screen) = (ids[0], ids[1]);
    assert_eq!(app.documents.active_id(), on_screen);

    let now = Instant::now();
    app.pump_on_screen_image_search();
    // Walk, then collect: the stub answers at once, a real job takes frames.
    assert!(app.pump_parked_image_search(now), "the slot was applied");

    let (src, credit) = slot_of(&mut app, parked);
    assert_eq!(src, url_for("alpha"), "the parked document's own query");
    assert_eq!(
        credit,
        Some(credit_for("alpha")),
        "the credit lands with the url, as on screen"
    );
    assert_eq!(app.documents.active_id(), on_screen, "no tab switch");
    assert_eq!(slot_of(&mut app, on_screen).0, url_for("beta"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn two_documents_with_one_node_id_never_receive_each_others_picture() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("collide", &["alpha", "beta"]);
    let now = Instant::now();
    app.pump_on_screen_image_search();
    app.pump_parked_image_search(now);
    assert_eq!(
        app.runs.image_search.pending_jobs() + app.documents.parked_image_jobs(),
        2,
        "one job per document, both for node `{SLOT}`"
    );

    // Answer in reverse of the order the jobs started.
    assert_eq!(release_held_searches(FINDER), 2);
    app.pump_parked_image_search(later(now, 1));
    app.pump_on_screen_image_search();

    assert_eq!(slot_of(&mut app, ids[0]).0, url_for("alpha"));
    assert_eq!(slot_of(&mut app, ids[1]).0, url_for("beta"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_tabs_keeps_the_jobs_of_the_document_that_leaves() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("switch", &["alpha", "beta"]);
    // `alpha` is shown first and starts its search on screen.
    assert!(app.activate_document_slot(0));
    app.pump_on_screen_image_search();
    assert_eq!(app.runs.image_search.pending_jobs(), 1);

    // The user moves on to `beta`: `alpha` is parked with its job still going.
    assert!(app.activate_document_slot(1));
    assert_eq!(
        app.documents.parked_image_jobs(),
        1,
        "the switch did not cancel the job"
    );
    app.pump_on_screen_image_search();
    release_held_searches(FINDER);
    // Both answers arrive while `beta` is on screen.
    app.pump_parked_image_search(Instant::now());
    app.pump_on_screen_image_search();

    assert_eq!(slot_of(&mut app, ids[0]).0, url_for("alpha"));
    assert_eq!(slot_of(&mut app, ids[1]).0, url_for("beta"));
    assert_eq!(app.documents.active_id(), ids[1]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_job_started_while_parked_still_lands_once_the_document_is_shown() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("shown-later", &["alpha", "beta"]);
    // `alpha` is parked: its search starts scene-less, through the lens.
    app.pump_parked_image_search(Instant::now());
    assert_eq!(app.documents.parked_image_jobs(), 1);

    // The user opens it; the answer arrives against the host's own scene and
    // must still match the slot's intent (the stale-result guard).
    assert!(app.activate_document_slot(0));
    release_held_searches(FINDER);
    assert!(app.pump_on_screen_image_search());
    assert_eq!(slot_of(&mut app, ids[0]).0, url_for("alpha"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_document_closed_mid_flight_drops_its_result() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("close", &["alpha", "beta"]);
    let now = Instant::now();
    app.pump_on_screen_image_search();
    app.pump_parked_image_search(now);
    assert_eq!(app.documents.parked_image_jobs(), 1);

    let index = app.documents.index_of(ids[0]).expect("parked tab");
    assert!(app.discard_document_tab(index));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(
        app.documents.parked_image_jobs(),
        0,
        "its session went along"
    );

    // Both answers come back; only the open document's is applied.
    release_held_searches(FINDER);
    app.pump_parked_image_search(later(now, 1));
    app.pump_on_screen_image_search();
    assert_eq!(slot_of(&mut app, ids[1]).0, url_for("beta"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn one_pump_walks_at_most_one_parked_document_and_the_rest_wait_their_turn() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, _ids, dir) = app_with_slot_documents("budget", &["a", "b", "c"]);
    let now = Instant::now();

    app.pump_parked_image_search(now);
    assert_eq!(app.documents.parked_image_jobs(), 1, "one walk per frame");

    app.pump_parked_image_search(later(now, 1));
    assert_eq!(
        app.documents.parked_image_jobs(),
        2,
        "the next frame's walk"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unedited_parked_document_is_left_alone_until_it_is_shown() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let dir = temp_dir("unedited");
    let mut app = DesktopApp::new(None);
    for (index, query) in ["alpha", "beta"].iter().enumerate() {
        let path = dir.join(format!("doc{index}.op"));
        save_to_path(&slot_state(query), &path).expect("fixture saves");
        assert!(app.open_document_tab(path));
    }
    // `doc0` is parked exactly as the file holds it (revision 0).
    app.pump_parked_image_search(Instant::now());
    assert_eq!(app.documents.parked_image_jobs(), 0);
    assert!(!app.documents.parked_image_work_outstanding());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn replacing_one_document_resets_only_its_own_session() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("reset", &["alpha", "beta"]);
    app.pump_on_screen_image_search();
    app.pump_parked_image_search(Instant::now());
    assert_eq!(app.documents.parked_image_jobs(), 1);
    assert_eq!(app.runs.image_search.pending_jobs(), 1);

    // The on-screen document is replaced: the parked session is not touched.
    app.reset_image_search_of(ids[1]);
    assert_eq!(app.runs.image_search.pending_jobs(), 0);
    assert_eq!(app.documents.parked_image_jobs(), 1);

    // The parked one is replaced: only its session goes.
    app.reset_image_search_of(ids[0]);
    assert_eq!(app.documents.parked_image_jobs(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_on_screen_document_is_still_filled_every_frame() {
    let _stub = StubGuard::new(StubbedSearch::Answer(FINDER));
    let (mut app, ids, dir) = app_with_slot_documents("on-screen", &["alpha", "beta"]);
    assert!(app.pump_image_enrichment() || app.pump_image_enrichment());
    assert_eq!(slot_of(&mut app, ids[1]).0, url_for("beta"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_scan_gap_and_the_unedited_rule_are_pure_decisions() {
    let session = crate::image_search_session::ImageSearchSession::new();
    let now = Instant::now();
    assert!(session.parked_work(0, 0, now).is_idle(), "revision 0");
    assert!(session.parked_work(3, 0, now).scan, "changed content");

    let mut walked = crate::image_search_session::ImageSearchSession::new();
    walked.note_parked_scan(now);
    assert!(
        !walked.parked_work(4, 0, later(now, 100)).scan,
        "inside the gap"
    );
    assert!(
        walked.parked_work(4, 0, later(now, 300)).scan,
        "past the gap"
    );
}

#[test]
fn an_occluded_window_still_fills_a_parked_documents_slot() {
    let _stub = StubGuard::new(StubbedSearch::Hold);
    let (mut app, ids, dir) = app_with_slot_documents("occluded", &["alpha", "beta"]);
    assert!(app.has_background_work(), "a changed parked document waits");

    // Only the wake path runs: it walks both documents' slots.
    app.last_background_pump = None;
    assert!(app.pump_background_when_redraw_starved().is_some());
    assert_eq!(release_held_searches(FINDER), 2);

    // Redraws are still missing 200 ms later: the wake path collects.
    app.last_background_pump = Some(Instant::now() - Duration::from_millis(200));
    assert_eq!(app.pump_background_when_redraw_starved(), Some(true));
    assert_eq!(slot_of(&mut app, ids[0]).0, url_for("alpha"));
    assert_eq!(slot_of(&mut app, ids[1]).0, url_for("beta"));
    let _ = std::fs::remove_dir_all(dir);
}
