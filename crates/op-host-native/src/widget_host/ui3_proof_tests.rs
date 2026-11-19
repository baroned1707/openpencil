//! Baseline proof scenes: the editor frame in both themes.

#[path = "ui3_proof_chat_tests.rs"]
mod chat;
#[path = "ui3_proof_font_download_tests.rs"]
mod font_download;
#[path = "ui3_proof_frame_tests.rs"]
mod frame;
#[path = "ui3_proof_home_tests.rs"]
mod home;
#[path = "ui3_proof_icons_tests.rs"]
mod icons;
#[path = "ui3_proof_images_tests.rs"]
mod images;
#[path = "ui3_proof_menus_tests.rs"]
mod menus;
#[path = "ui3_proof_open_marks_tests.rs"]
mod open_marks;
#[path = "ui3_proof_open_project_tests.rs"]
mod open_project;
#[path = "ui3_proof_open_project_actions_tests.rs"]
mod open_project_actions;
#[path = "ui3_proof_info_bar_tests.rs"]
mod info_bar;
#[path = "ui3_proof_panels_tests.rs"]
mod panels;
#[path = "ui3_proof_projects_tests.rs"]
mod projects;
#[path = "ui3_proof_quick_switcher_tests.rs"]
mod quick_switcher;
#[path = "ui3_proof_settings_tests.rs"]
mod settings;
#[path = "ui3_proof_style_suggest_tests.rs"]
mod style_suggest;
#[path = "document_swap_render_tests.rs"]
mod swap_render;
#[path = "ui3_proof_usage_tests.rs"]
mod usage;

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
