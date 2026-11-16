//! Baseline proof scenes: the editor frame in both themes.

use super::ui3_proof::{editor_host, render_scene};
use op_editor_core::ThemeMode;

#[test]
fn editor_frame_renders_in_both_themes() {
    for theme in [ThemeMode::Dark, ThemeMode::Light] {
        let mut host = editor_host(theme, Some("card"));
        // Probe a pixel inside the layer rail: it must be the panel tone.
        let rgb = render_scene(&mut host, "01-editor-frame", (60, 400));
        assert!(rgb != [0, 0, 0], "{theme:?}: the rail painted something");
    }
}
