//! Two open documents, one on screen: an MCP-style write addressed to the
//! parked one must land in it and nowhere else, and the on-screen document
//! must keep working untouched.

use super::*;
use crate::mcp_document_resolver::DesktopDocumentResolver;
use op_editor_core::{DocumentId, PenNodeExt};
use op_host_services::doc_io::save_to_path;
use op_host_services::mcp_live::{DocumentResolver, ResolveError};
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-mcpdoc-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `a.op` (parked) and `b.op` (on screen).
fn two_documents(tag: &str) -> (DesktopApp, DocumentId, DocumentId, PathBuf) {
    let dir = temp_dir(tag);
    let mut app = DesktopApp::new(None);
    for name in ["a.op", "b.op"] {
        let path = dir.join(name);
        save_to_path(&op_editor_core::EditorState::starter(), &path).unwrap();
        assert!(app.open_document_tab(path));
    }
    let a = app.documents.id_at(0).unwrap();
    let b = app.documents.id_at(1).unwrap();
    assert_eq!(app.documents.active_id(), b, "b is on screen, a is parked");
    (app, a, b, dir)
}

fn recolour(state: &mut op_editor_core::EditorState, hex: &str) {
    let id = state.doc.children[0].id_str().to_string();
    state.set_single_selection(op_editor_core::NodeId::new(&id));
    assert!(state.set_selected_color(true, hex));
    state.mark_document_changed();
}

fn first_frame(state: &op_editor_core::EditorState) -> String {
    serde_json::to_string(&state.doc.children[0]).unwrap()
}

#[test]
fn a_write_to_a_parked_document_never_touches_the_one_on_screen() {
    let (mut app, a, b, dir) = two_documents("park-write");
    let before_b = first_frame(app.host.editor_state());
    let b_epoch = app.host.document_epoch();
    let listings = app.mcp_document_listings();

    let mut seen_epoch = None;
    {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        let result = resolver.with_document(a, &mut |state, epoch| {
            seen_epoch = Some(epoch);
            recolour(state, "#112233");
        });
        assert_eq!(result, Ok(()));
    }

    assert_ne!(
        seen_epoch,
        Some(b_epoch),
        "the call saw A's own epoch, not B's"
    );
    assert_eq!(app.host.document_epoch(), b_epoch, "B's epoch is back");
    assert_eq!(
        first_frame(app.host.editor_state()),
        before_b,
        "the on-screen document B is exactly as it was"
    );
    assert!(
        app.documents.is_dirty(0, false),
        "A is the one that changed"
    );
    assert!(!app.document_is_dirty(), "B stayed clean");
    assert_eq!(app.documents.active_id(), b);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn writes_to_two_documents_stay_in_their_own_document() {
    let (mut app, a, b, dir) = two_documents("two-writes");
    let listings = app.mcp_document_listings();
    {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        resolver
            .with_document(a, &mut |state, _| recolour(state, "#AA0000"))
            .unwrap();
        resolver
            .with_document(b, &mut |state, _| recolour(state, "#00BB00"))
            .unwrap();
    }
    let on_screen = first_frame(app.host.editor_state()).to_uppercase();
    assert!(on_screen.contains("00BB00"), "{on_screen}");
    assert!(!on_screen.contains("AA0000"));

    assert!(app.activate_document_slot(0));
    let now_a = first_frame(app.host.editor_state()).to_uppercase();
    assert!(now_a.contains("AA0000"), "{now_a}");
    assert!(!now_a.contains("00BB00"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_document_is_refused_without_side_effects() {
    let (mut app, _a, _b, dir) = two_documents("unknown");
    let listings = app.mcp_document_listings();
    let before = first_frame(app.host.editor_state());
    {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        let result = resolver.with_document(DocumentId(9999), &mut |_, _| panic!("must not run"));
        assert_eq!(result, Err(ResolveError::Unknown));
    }
    assert_eq!(first_frame(app.host.editor_state()), before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_parked_document_is_back_in_its_slot_after_the_call() {
    let (mut app, a, _b, dir) = two_documents("restore");
    let listings = app.mcp_document_listings();
    {
        let mut resolver =
            DesktopDocumentResolver::new(&mut app.host, &mut app.documents, listings);
        resolver.with_document(a, &mut |_, _| {}).unwrap();
    }
    assert!(app.activate_document_slot(0));
    assert_eq!(app.documents.active_id(), a);
    let _ = std::fs::remove_dir_all(dir);
}
