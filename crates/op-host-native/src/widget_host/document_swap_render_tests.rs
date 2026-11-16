//! End-to-end pixels: what the canvas actually shows after switching documents.
//!
//! The data-level swap tests prove the scene cache travels with its document.
//! These render the real editor composition and read the canvas, because a
//! stale cache would show up there first — as the previous document's colour.

use super::super::document_swap::ParkedDocument;
use super::super::ui3_proof::{render_probes, PROOF_H, PROOF_W};
use super::super::WidgetHostNative;
use jian_ops_schema::image_thumbs::ImageThumbSnapshot;

/// Canvas centre for the proof viewport (right of the 240px rail, left of the
/// 256px property rail, below the top bar).
const CANVAS_CENTRE: (i32, i32) = (700, 470);

fn state_filled(frame_id: &str, hex: &str) -> op_editor_core::EditorState {
    let src = format!(
        r##"{{"version":"{}","children":[{{"type":"frame","id":"{frame_id}","name":"{frame_id}",
            "x":0,"y":0,"width":640,"height":420,
            "fill":[{{"type":"solid","color":"{hex}"}}],"children":[]}}]}}"##,
        env!("CARGO_PKG_VERSION")
    );
    let doc = jian_ops_schema::load_str(&src).expect("fixture").value;
    let mut state = op_editor_core::EditorState::from_document(doc);
    state.editor_ui.home.hide();
    state
}

fn canvas_colour(host: &mut WidgetHostNative, name: &str) -> [u8; 3] {
    host.zoom_to_fit(PROOF_W, PROOF_H);
    render_probes(host, name, &[CANVAS_CENTRE])[0]
}

fn host_showing(frame_id: &str, hex: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    assert!(host.swap_document(&mut ParkedDocument::from_state(
        state_filled(frame_id, hex),
        ImageThumbSnapshot::default(),
    )));
    host.mark_dirty();
    host
}

#[test]
fn canvas_shows_the_switched_to_document_and_not_a_stale_cache() {
    let mut host = host_showing("docA", "#FF0000");
    assert_eq!(
        canvas_colour(&mut host, "80-swap-a"),
        [255, 0, 0],
        "A is red"
    );

    // B has the same revision and generation as A did; only its content differs.
    let mut parked = ParkedDocument::from_state(
        state_filled("docB", "#00FF00"),
        ImageThumbSnapshot::default(),
    );
    assert!(host.swap_document(&mut parked));
    assert_eq!(
        canvas_colour(&mut host, "80-swap-b"),
        [0, 255, 0],
        "B is green: the canvas must not keep painting A's cached scene"
    );

    // Back to A: the parked slot now holds A, with its own scene.
    assert!(host.swap_document(&mut parked));
    assert_eq!(
        canvas_colour(&mut host, "80-swap-a-again"),
        [255, 0, 0],
        "A is red again after the round trip"
    );
}

#[test]
fn an_edit_in_one_document_never_shows_in_the_other() {
    let mut host = host_showing("docA", "#FF0000");
    let mut parked = ParkedDocument::from_state(
        state_filled("docB", "#00FF00"),
        ImageThumbSnapshot::default(),
    );

    // Recolour A (the one on screen) through the real selection + colour path.
    host.editor_state_mut()
        .set_single_selection(op_editor_core::NodeId::new("docA"));
    assert!(host.editor_state_mut().set_selected_color(true, "#0000FF"));
    host.mark_dirty();
    assert_eq!(
        canvas_colour(&mut host, "80-edit-a"),
        [0, 0, 255],
        "A is blue"
    );
    let dirty_before_swap = host.editor_state().is_dirty();

    assert!(host.swap_document(&mut parked));
    assert_eq!(
        canvas_colour(&mut host, "80-edit-b"),
        [0, 255, 0],
        "B is untouched by the edit made in A"
    );

    assert!(host.swap_document(&mut parked));
    assert_eq!(
        canvas_colour(&mut host, "80-edit-a-back"),
        [0, 0, 255],
        "A kept its edit while parked"
    );
    assert_eq!(
        host.editor_state().is_dirty(),
        dirty_before_swap,
        "swapping neither loses nor invents unsaved changes"
    );
}

#[test]
fn each_document_has_its_own_epoch_and_selection() {
    let mut host = host_showing("docA", "#FF0000");
    host.editor_state_mut()
        .set_single_selection(op_editor_core::NodeId::new("docA"));
    let epoch_a = host.document_epoch();

    let mut parked = ParkedDocument::from_state(
        state_filled("docB", "#00FF00"),
        ImageThumbSnapshot::default(),
    );
    let epoch_b = parked.epoch();
    assert!(host.swap_document(&mut parked));
    assert_eq!(host.document_epoch(), epoch_b);
    assert_ne!(
        epoch_a, epoch_b,
        "epochs are never shared between documents"
    );
    assert!(
        host.editor_state().selected_node().is_none(),
        "B has no selection; A's selection did not follow the swap"
    );

    assert!(host.swap_document(&mut parked));
    assert_eq!(host.document_epoch(), epoch_a, "A gets its own epoch back");
    assert!(host.editor_state().selected_node().is_some());
}
