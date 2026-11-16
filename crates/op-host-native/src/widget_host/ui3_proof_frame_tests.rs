//! Proof scenes for the Figma UI3 restyle — frame wave.
//!
//! One `#[test]` per surface; each renders the editor frame in both themes
//! so `.proof-of-work/` gets `10-frame-<scene>-<dark|light>.png`. Every
//! scene also asserts the borderless rule on the pixels it just painted: the
//! rail and the canvas meet with no hairline between them.

use crate::widget_host::ui3_proof::{editor_host, render_probes, rgb8};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{PropertyTab, ThemeMode};
use op_editor_ui::Theme;

const THEMES: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

/// Render one scene in both themes and check the surface tones.
fn scene(name: &str, selected: Option<&str>, setup: impl Fn(&mut WidgetHostNative)) {
    for mode in THEMES {
        let theme = match mode {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        };
        let mut host = editor_host(mode, selected);
        setup(&mut host);
        host.mark_dirty();
        let file = format!("10-frame-{name}");
        // Left rail interior, either side of the rail/canvas seam, the canvas
        // itself, and the right rail's left edge (x = panel origin).
        let probes = [
            (60, 600),
            (238, 600),
            (239, 600),
            (241, 600),
            (700, 120),
            (1186, 700),
        ];
        let px = render_probes(&mut host, &file, &probes);
        assert_eq!(px[0], rgb8(theme.card), "{mode:?} {name}: rail tone");
        // No hairline at the seam: the rail tone runs right up to the canvas.
        assert_eq!(px[1], rgb8(theme.card), "{mode:?} {name}: rail edge");
        assert_eq!(px[2], rgb8(theme.card), "{mode:?} {name}: rail seam");
        assert_eq!(px[3], rgb8(theme.canvas_surface), "{mode:?} {name}: canvas");
        assert_eq!(px[4], rgb8(theme.canvas_surface), "{mode:?} {name}: canvas");
        if selected.is_some() {
            assert_eq!(px[5], rgb8(theme.card), "{mode:?} {name}: right rail seam");
        }
    }
}

#[test]
fn nothing_selected() {
    scene("nothing-selected", None, |_| {});
}

#[test]
fn frame_selected() {
    scene("frame-selected", Some("board"), |_| {});
}

#[test]
fn text_selected() {
    scene("text-selected", Some("title"), |_| {});
}

#[test]
fn shape_picker_open() {
    scene("shape-picker", Some("card"), |host| {
        op_editor_core::host_press_transitions::toggle_shape_picker(
            &mut host.editor_state_mut().editor_ui,
        );
    });
}

#[test]
fn fill_picker_open() {
    scene("fill-picker", Some("card"), |host| {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.fill_type_picker.open = true;
        ui.fill_type_picker_index = 0;
    });
}

#[test]
fn mode_popover_open() {
    scene("mode-popover", Some("card"), |host| {
        host.editor_state_mut().editor_ui.stroke_mode_popover_open = true;
    });
}

#[test]
fn layer_row_hovered() {
    scene("layers-hover", Some("card"), |host| {
        host.editor_state_mut().editor_ui.hovered_layer_id =
            Some(op_editor_core::NodeId::new("button"));
    });
}

#[test]
fn design_tab() {
    scene("design-tab", Some("card"), |host| {
        host.editor_state_mut()
            .editor_ui
            .set_property_tab(PropertyTab::Design);
    });
}

#[test]
fn code_tab() {
    scene("code-tab", Some("card"), |host| {
        host.editor_state_mut()
            .editor_ui
            .set_property_tab(PropertyTab::Code);
    });
}
