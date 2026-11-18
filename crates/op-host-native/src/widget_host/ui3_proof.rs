//! Headless proof renders for the Figma UI3 restyle.
//!
//! `render_scene` paints the real editor composition into an off-screen
//! raster surface and writes a PNG — no window, no GL, nothing on screen.
//! It only writes when `OP_PROOF_DIR` names a directory, so a normal
//! `cargo test` stays side-effect free. Each restyle wave adds its own
//! `ui3_proof_<wave>_tests.rs` sibling that calls the helpers below.
//!
//!   OP_PROOF_DIR=$PWD/.proof-of-work cargo test -p op-host-native \
//!       --features gl-host ui3_proof

use super::WidgetHostNative;
use op_editor_core::ThemeMode;

pub(super) const PROOF_W: f32 = 1440.0;
pub(super) const PROOF_H: f32 = 900.0;

/// A small editor document: one artboard with a card, a button and text, so
/// every rail (layers, properties, canvas) has something to show.
pub(super) fn sample_document() -> jian_ops_schema::PenDocument {
    jian_ops_schema::load_str(
        r##"{"version":"1.0.0","children":[
          {"type":"frame","id":"board","name":"Dashboard","x":0,"y":0,"width":640,"height":420,
           "fill":[{"type":"solid","color":"#FFFFFF"}],"cornerRadius":16,"children":[
            {"type":"rectangle","id":"card","name":"Card","x":32,"y":32,"width":280,"height":160,
             "cornerRadius":12,"fill":[{"type":"solid","color":"#F5F5F5"}]},
            {"type":"rectangle","id":"button","name":"Button","x":32,"y":220,"width":120,"height":40,
             "cornerRadius":8,"fill":[{"type":"solid","color":"#0D99FF"}]},
            {"type":"text","id":"title","name":"Title","x":336,"y":40,"width":260,"height":32,
             "content":"Figma-style UI","fontSize":24,"fontWeight":"700",
             "fill":[{"type":"solid","color":"#1A1A1A"}]}
          ]}
        ]}"##,
    )
    .expect("sample document parses")
    .value
}

/// An editor host showing `sample_document` (Home hidden), `selected` picked.
pub(super) fn editor_host(theme: ThemeMode, selected: Option<&str>) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    *host.editor_state_mut() = op_editor_core::EditorState::from_document(sample_document());
    host.editor_state_mut().editor_ui.home.hide();
    host.editor_state_mut().editor_ui.theme_mode = theme;
    if let Some(id) = selected {
        host.editor_state_mut()
            .set_single_selection(op_editor_core::NodeId::new(id));
    }
    // The state was replaced behind the host's back: flag the scene stale,
    // then frame the document so the artboard is visible in the canvas.
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

/// Paint `host` and write `<OP_PROOF_DIR>/<name>-<dark|light>.png`.
/// Returns the pixel at (x, y) so a test can still assert on the render.
pub(super) fn render_scene(host: &mut WidgetHostNative, name: &str, probe: (i32, i32)) -> [u8; 3] {
    render_probes(host, name, &[probe])[0]
}

/// `render_scene` with several probe points (one RGB triple per probe).
pub(super) fn render_probes(
    host: &mut WidgetHostNative,
    name: &str,
    probes: &[(i32, i32)],
) -> Vec<[u8; 3]> {
    render_probes_at(host, name, probes, PROOF_W, PROOF_H)
}

/// [`render_probes`] on a `width` × `height` window — for scenes about the
/// window width (the info bar at 640 / 960 px) or a band of the chrome.
pub(super) fn render_probes_at(
    host: &mut WidgetHostNative,
    name: &str,
    probes: &[(i32, i32)],
    width: f32,
    height: f32,
) -> Vec<[u8; 3]> {
    host.last_viewport_w = width;
    host.last_viewport_h = height;
    let mut backend = crate::backend::NativeBackend::with_dpi(1.0);
    let mut surface =
        skia_safe::surfaces::raster_n32_premul((width as i32, height as i32)).unwrap();
    // A few frames so first-frame work (fonts, layout caches) settles.
    for _ in 0..3 {
        let mut frame = crate::backend::NativeFrameBackend::new(&mut backend, surface.canvas());
        host.paint(&mut frame, width, height);
    }
    if let Ok(dir) = std::env::var("OP_PROOF_DIR") {
        let suffix = match host.editor_state().editor_ui.theme_mode {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        };
        let bytes = surface
            .image_snapshot()
            .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
            .expect("png encodes");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}-{suffix}.png")),
            bytes.as_bytes(),
        )
        .unwrap();
    }
    let pixels = surface.peek_pixels().unwrap();
    probes
        .iter()
        .map(|&probe| {
            let c = pixels.get_color(probe);
            [c.r(), c.g(), c.b()]
        })
        .collect()
}

/// An 8-bit RGB triple for a theme colour, for comparing against probes.
pub(super) fn rgb8(color: op_editor_ui::Color) -> [u8; 3] {
    let byte = |v: f32| (v * 255.0).round() as u8;
    [byte(color.r), byte(color.g), byte(color.b)]
}
