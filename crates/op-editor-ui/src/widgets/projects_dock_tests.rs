//! Paint tests for the Projects dock over the capture backend: what the
//! open header and the collapsed strip put on the canvas.

use super::*;
use crate::widgets::projects_panel::ProjectsPanelText;
use crate::widgets::projects_panel_flow::projects_panel_flow_tests::{project, shop};
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::ProjectView;

const TEXT: ProjectsPanelText<'static> = ProjectsPanelText {
    empty: "No projects yet",
    new_design: "New design",
    needs_review: "{{count}} need review",
    running: "{{count}} running",
    open_project: "Open Project…",
    open_hint: "Choose a folder, or drop one here",
    close: "Close project",
    refresh: "Refresh",
    scanning: "Scanning…",
    slow: "Slow folder: showing what was found",
    unreadable: "Cannot read this folder",
    truncated: "{{count}} more not shown",
    no_designs: "No .op files here yet",
    fig_convert: "Convert",
    fig_converted: "Converted",
    fig_converting: "Converting…",
    fig_failed: "Failed",
    fig_convert_all: "Convert all ({{count}})",
    fig_convert_tip: "Convert to .op next to this file",
    fig_converted_tip: "Open the converted .op file",
    fig_failed_tip: "Conversion failed: {{reason}}. Click to retry.",
    fig_convert_all_tip: "Convert every .fig file here to .op next to it",
    rename: "Rename",
    rename_placeholder: "Name",
    rename_invalid: "This name can't be used",
    rename_exists: "A file with that name already exists",
};

fn paint(
    projects: &[ProjectView],
    open: bool,
    toggle_hover: bool,
    theme: &Theme,
) -> CaptureBackend {
    let rect = Rect::xywh(1_168.0, 40.0, if open { 232.0 } else { 28.0 }, 760.0);
    let layout = ProjectsDockLayout::new(projects, rect, open, 0.0);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsDock {
        list: ProjectsPanel {
            projects,
            hover: None,
            rename: None,
            now_ms: 0,
            text: &TEXT,
        },
        title: "Projects",
        toggle_hover,
        open_project_hover: false,
        has_projects: !projects.is_empty(),
    }
    .paint(&mut cx, &layout, theme);
    backend
}

fn painted(backend: &CaptureBackend, text: &str) -> bool {
    backend.texts.iter().any(|(drawn, _)| drawn == text)
}

#[test]
fn the_open_dock_paints_its_title_and_the_project_list() {
    let theme = Theme::dark();
    let backend = paint(&shop(), true, false, &theme);
    assert!(painted(&backend, "Projects"));
    assert!(
        painted(&backend, "shop"),
        "the list paints under the header"
    );
    assert!(painted(&backend, "checkout.op"));
}

#[test]
fn the_collapsed_strip_shows_the_project_count_and_no_list() {
    let theme = Theme::dark();
    let backend = paint(&shop(), false, false, &theme);
    assert!(painted(&backend, "2"), "two projects -> a `2` badge");
    assert!(!painted(&backend, "Projects"));
    assert!(!painted(&backend, "shop"));
    // The folder glyph and the chevron: two strokes' worth of icon paths.
    assert!(backend.svg_strokes.len() >= 2);
}

#[test]
fn a_huge_project_count_caps_the_badge() {
    let theme = Theme::dark();
    let many: Vec<ProjectView> = (0..120)
        .map(|n| project(&format!("p{n}"), Vec::new(), false))
        .collect();
    let backend = paint(&many, false, false, &theme);
    assert!(painted(&backend, "99+"));
}

#[test]
fn hovering_the_chevron_washes_it() {
    let theme = Theme::dark();
    let washes = |backend: &CaptureBackend| {
        backend
            .rect_fills
            .iter()
            .filter(|(_, color)| *color == theme.button_hover)
            .count()
    };
    assert_eq!(washes(&paint(&shop(), true, false, &theme)), 0);
    assert_eq!(washes(&paint(&shop(), true, true, &theme)), 1);
    assert_eq!(washes(&paint(&shop(), false, true, &theme)), 1);
}
