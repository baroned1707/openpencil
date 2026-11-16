//! Switching documents moves the document, its derived caches, its thumbnails
//! and its epoch together, and leaves app-level state alone.

use super::*;
use jian_ops_schema::image_thumbs::{store_thumb, thumb_for};
use std::sync::Mutex;

/// The thumbnail registry is process-global; tests that fill it take turns.
static THUMB_REGISTRY: Mutex<()> = Mutex::new(());

fn lock_thumbs() -> std::sync::MutexGuard<'static, ()> {
    THUMB_REGISTRY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A one-frame document whose frame id names it, so a scene built from it
/// can be told apart from every other test document.
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
    let ok = host.swap_document(&mut ParkedDocument::from_state(
        state_named(frame_id),
        ImageThumbSnapshot::default(),
    ));
    assert!(ok);
    host
}

fn scene_has(host: &mut WidgetHostNative, frame_id: &str) -> bool {
    host.layout_scene()
        .active_page()
        .is_some_and(|page| page.find(frame_id).is_some())
}

#[test]
fn caches_travel_with_their_document() {
    let mut host = host_showing("docA");
    assert!(
        scene_has(&mut host, "docA"),
        "A's scene is built and cached"
    );

    // B starts at revision 0 and generation 0, exactly like A. A cache keyed
    // by revision would call B's inputs unchanged and keep painting A.
    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    assert!(host.swap_document(&mut parked_b));
    assert!(scene_has(&mut host, "docB"), "B gets its own scene");
    assert!(
        !scene_has(&mut host, "docA"),
        "A's scene is not served for B"
    );

    assert!(host.swap_document(&mut parked_b));
    assert!(scene_has(&mut host, "docA"), "A's scene comes back with A");
    assert!(!scene_has(&mut host, "docB"));
}

#[test]
fn the_layer_panel_owner_travels_with_its_document() {
    let mut host = host_showing("docA");
    let owner_a = host.layer_panel_owner;
    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    assert!(host.swap_document(&mut parked_b));
    let owner_b = host.layer_panel_owner;
    assert_ne!(
        owner_a, owner_b,
        "revision-0 documents must not share a cache"
    );
    assert!(host.swap_document(&mut parked_b));
    assert_eq!(host.layer_panel_owner, owner_a);
}

#[test]
fn epoch_follows_the_document_and_is_never_reused() {
    let mut host = host_showing("docA");
    let epoch_a = host.document_epoch();
    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    let epoch_b = parked_b.epoch();
    assert_ne!(epoch_a, epoch_b);

    assert!(host.swap_document(&mut parked_b));
    assert_eq!(host.document_epoch(), epoch_b);
    assert_eq!(parked_b.epoch(), epoch_a, "the parked slot keeps A's epoch");

    // A whole-document replacement allocates a fresh epoch rather than
    // `epoch + 1`, which could land on the other open document's value.
    let replaced = host.replace_editor_state(state_named("docC"));
    assert!(replaced);
    assert_ne!(host.document_epoch(), epoch_a);
    assert_ne!(host.document_epoch(), epoch_b);
}

#[test]
fn app_level_state_does_not_follow_a_host_swap() {
    let mut host = host_showing("docA");
    host.editor_state_mut().editor_ui.locale = op_editor_core::Locale::Ja;
    host.editor_state_mut().editor_ui.touch = true;
    host.editor_state_mut().chat.title = "design chat".to_owned();

    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    assert!(host.swap_document(&mut parked_b));
    assert_eq!(
        host.editor_state().editor_ui.locale,
        op_editor_core::Locale::Ja
    );
    assert!(host.editor_state().editor_ui.touch);
    assert_eq!(host.editor_state().chat.title, "design chat");
}

#[test]
fn thumbs_follow_slot() {
    let _guard = lock_thumbs();
    const ONLY_A: u64 = 0xD0C5_0001;
    const ONLY_B: u64 = 0xD0C5_0002;
    let mut host = host_showing("docA");
    store_thumb(ONLY_A, vec![1_u8, 2, 3]);

    // Loading B must neither leak into A's registry nor lose it.
    let (state_b, thumbs_b) = capture_loaded_thumbnails(|| {
        store_thumb(ONLY_B, vec![9_u8, 9]);
        state_named("docB")
    });
    assert!(thumb_for(ONLY_A).is_some(), "A's registry was restored");
    assert!(
        thumb_for(ONLY_B).is_none(),
        "B's thumbnails stayed out of A"
    );

    let mut parked_b = ParkedDocument::from_state(state_b, thumbs_b);
    assert!(host.swap_document(&mut parked_b));
    assert!(thumb_for(ONLY_B).is_some(), "B's thumbnails are live");
    assert!(thumb_for(ONLY_A).is_none(), "A's thumbnails are parked");

    assert!(host.swap_document(&mut parked_b));
    assert!(thumb_for(ONLY_A).is_some());
    assert!(thumb_for(ONLY_B).is_none());
}

#[test]
fn a_swap_leaves_the_incoming_document_untouched() {
    let mut host = host_showing("docA");
    let mut parked_b = ParkedDocument::from_state(state_named("docB"), Default::default());
    assert!(host.swap_document(&mut parked_b));
    assert_eq!(host.editor_state().document_revision(), 0, "B is untouched");
    assert!(!parked_b.is_dirty());
}
