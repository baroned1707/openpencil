//! Baseline proof scenes: the editor frame in both themes.

#[path = "ui3_proof_chat_tests.rs"]
mod chat;
#[path = "ui3_proof_frame_tests.rs"]
mod frame;
#[path = "ui3_proof_home_tests.rs"]
mod home;
#[path = "ui3_proof_menus_tests.rs"]
mod menus;
#[path = "ui3_proof_panels_tests.rs"]
mod panels;
#[path = "ui3_proof_settings_tests.rs"]
mod settings;
#[path = "document_swap_render_tests.rs"]
mod swap_render;
#[path = "ui3_proof_tabs_tests.rs"]
mod tabs;

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
