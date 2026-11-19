//! Proof scene for `icon_font` glyph resolution: a row of Lucide icons that the
//! editor's hand-copied chrome table does NOT have, resolved through the
//! embedded catalog on the real canvas, once as dark ink on a light board and
//! once as light ink on a dark board, in both editor themes. The last two
//! cells are a Feather-only glyph (a `lucide` name falls back to it) and an
//! unknown name, which paints nothing.
//!
//!   OP_PROOF_DIR=$PWD/.proof-of-work cargo test -p op-host-native \
//!       --features gl-host ui3_proof_icons
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use super::super::ui3_proof::{render_probes_at, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::ThemeMode;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// Lucide glyphs outside the ~75 hand-copied ones (checked by a catalog test).
const LUCIDE_NAMES: [&str; 14] = [
    "binoculars",
    "croissant",
    "drum",
    "flower-2",
    "ghost",
    "guitar",
    "hand-coins",
    "microscope",
    "pizza",
    "telescope",
    "trophy",
    "wine",
    "bell-ring",
    "anchor",
];

fn icon_cell(id: usize, name: &str, family: &str, ink: &str) -> String {
    format!(
        r##"{{"type":"icon_font","id":"icon-{id}","name":"{name}","iconFontName":"{name}",
            "iconFontFamily":"{family}","width":44,"height":44,
            "fill":[{{"type":"solid","color":"{ink}"}}]}}"##
    )
}

/// One board: a row of the glyphs on `background`, drawn in `ink`.
fn board(id: &str, background: &str, ink: &str) -> String {
    let mut cells: Vec<String> = LUCIDE_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| icon_cell(index, name, "lucide", ink))
        .collect();
    cells.push(icon_cell(14, "codepen", "feather", ink));
    cells.push(icon_cell(15, "zzzzqqqq", "lucide", ink));
    format!(
        r##"{{"type":"frame","id":"{id}","name":"{id}","x":0,"y":0,"width":940,"height":100,
            "layout":"horizontal","gap":12,"padding":24,"alignItems":"center",
            "fill":[{{"type":"solid","color":"{background}"}}],"children":[{}]}}"##,
        cells.join(",")
    )
}

/// An editor showing ONE board. (Two top-level boards in one document paint
/// only the last board's children in this headless harness, so each board is
/// its own scene.)
fn icons_host(theme: ThemeMode, board_json: &str) -> WidgetHostNative {
    let document = format!(r#"{{"version":"1.0.0","children":[{board_json}]}}"#);
    let mut host = WidgetHostNative::new();
    *host.editor_state_mut() = op_editor_core::EditorState::from_document(
        jian_ops_schema::load_str(&document)
            .expect("icon proof document parses")
            .value,
    );
    host.editor_state_mut().editor_ui.home.hide();
    host.editor_state_mut().editor_ui.theme_mode = theme;
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

/// How many probed pixels are (within `tolerance` per channel) the ink colour.
fn ink_pixels(pixels: &[[u8; 3]], ink: [u8; 3], tolerance: i32) -> usize {
    pixels
        .iter()
        .filter(|pixel| {
            pixel
                .iter()
                .zip(ink)
                .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= tolerance)
        })
        .count()
}

#[test]
fn icon_font_resolves_lucide_glyphs_outside_the_chrome_table_proof() {
    // (scene name, board background, ink): dark ink on a light board and light
    // ink on a dark board. The inks are colours the editor chrome never uses
    // in the probed canvas area, so every counted pixel is glyph stroke.
    let scenes = [
        (
            "98-icon-font-lucide-on-light",
            "#FFFFFF",
            [0x25, 0x63, 0xEB],
        ),
        ("98-icon-font-lucide-on-dark", "#111111", [0xF5, 0xF5, 0xF5]),
    ];
    for mode in BOTH {
        for (name, background, ink) in scenes {
            let ink_hex = format!("#{:02X}{:02X}{:02X}", ink[0], ink[1], ink[2]);
            let mut host = icons_host(mode, &board("board", background, &ink_hex));
            let probes: Vec<(i32, i32)> = (330..1400)
                .step_by(3)
                .flat_map(|x| (250..700).step_by(3).map(move |y| (x, y)))
                .collect();
            let pixels = render_probes_at(&mut host, name, &probes, PROOF_W, PROOF_H);
            let painted = ink_pixels(&pixels, ink, 4);
            // 15 glyphs of a few hundred stroke px each at this sampling: far above the
            // few pixels a stray antialiased edge could give.
            assert!(
                painted > 150,
                "{mode:?} {name}: only {painted} glyph pixels"
            );
        }
    }
}
