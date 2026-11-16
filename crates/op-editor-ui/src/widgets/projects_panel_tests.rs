//! Paint tests for the Projects tab, over the capture backend: which fills
//! and strings reach the canvas for a given set of project rows.

use super::*;
use crate::widgets::projects_panel_flow::projects_panel_flow_tests::{file, project, shop, RAIL};
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::{DocumentId, DocumentTabStatus};

const TEXT: ProjectsPanelText<'static> = ProjectsPanelText {
    empty: "No projects yet",
    new_design: "New design",
    needs_review: "{{count}} need review",
    running: "{{count}} running",
};

fn paint(
    projects: &[ProjectView],
    hover: Option<ProjectRowTarget>,
    panel: Rect,
    theme: &Theme,
) -> CaptureBackend {
    let layout = ProjectsPanelLayout::new(projects, panel, 0.0);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects,
        hover,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, theme);
    backend
}

fn body() -> Rect {
    Rect::xywh(0.0, 48.0 + 44.0, RAIL.size.x, RAIL.size.y - 44.0)
}

fn painted(backend: &CaptureBackend, text: &str) -> bool {
    backend.texts.iter().any(|(drawn, _)| drawn == text)
}

fn fills_with(backend: &CaptureBackend, color: Color) -> usize {
    backend
        .rect_fills
        .iter()
        .filter(|(_, fill)| *fill == color)
        .count()
}

#[test]
fn active_file_row_is_visually_distinct() {
    let theme = Theme::dark();
    let mut projects = shop();
    projects[0].files[0].active = true;
    let backend = paint(&projects, None, body(), &theme);
    assert_eq!(
        fills_with(&backend, theme.row_selected_primary),
        1,
        "exactly the active file is filled"
    );

    projects[0].files[0].active = false;
    let backend = paint(&projects, None, body(), &theme);
    assert_eq!(fills_with(&backend, theme.row_selected_primary), 0);
}

#[test]
fn hovering_a_file_washes_it_with_the_accent_tone() {
    let theme = Theme::dark();
    let backend = paint(&shop(), Some(ProjectRowTarget::File(0, 1)), body(), &theme);
    assert_eq!(fills_with(&backend, theme.accent), 1);
}

#[test]
fn the_plus_shows_only_on_the_hovered_header() {
    let theme = Theme::dark();
    let plus_glyphs = |backend: &CaptureBackend| {
        backend
            .svg_strokes
            .iter()
            .filter(|(d, ..)| Icon::Plus.paths().contains(&d.as_str()))
            .count()
    };
    let idle = paint(&shop(), None, body(), &theme);
    assert_eq!(plus_glyphs(&idle), 0);
    let hovered = paint(&shop(), Some(ProjectRowTarget::Header(0)), body(), &theme);
    assert!(plus_glyphs(&hovered) > 0, "the plus glyph is drawn");
    // The chip gives way to the button on the hovered header.
    assert!(!painted(&hovered, "2"));
    assert!(painted(&idle, "2"));
}

#[test]
fn needs_review_aggregate_is_painted_in_the_warning_tone() {
    let theme = Theme::dark();
    let mut projects = shop();
    projects[0].files[0].status = DocumentTabStatus::NeedsReview;
    projects[0].files[1].status = DocumentTabStatus::NeedsReview;
    let backend = paint(&projects, None, body(), &theme);
    assert!(painted(&backend, "2 need review"));
    // One dot per file, in the warning tone.
    let dots = backend
        .round_fills
        .iter()
        .filter(|(_, _, color)| *color == theme.status_warning)
        .count();
    assert_eq!(dots, 2);
}

#[test]
fn running_aggregate_shows_when_nothing_needs_review() {
    let theme = Theme::dark();
    let mut projects = shop();
    projects[0].files[1].status = DocumentTabStatus::Running;
    let backend = paint(&projects, None, body(), &theme);
    assert!(painted(&backend, "1 running"));
    assert!(!painted(&backend, "1 need review"));
}

#[test]
fn idle_project_shows_its_file_count() {
    let theme = Theme::dark();
    let backend = paint(&shop(), None, body(), &theme);
    assert!(painted(&backend, "2"));
    assert!(painted(&backend, "1"));
}

#[test]
fn each_status_has_its_own_dot_tone() {
    let theme = Theme::dark();
    let mut files = vec![
        file("a.op", Some(1)),
        file("b.op", Some(2)),
        file("c.op", Some(3)),
        file("d.op", Some(4)),
        file("e.op", Some(5)),
    ];
    for (f, status) in files.iter_mut().zip([
        DocumentTabStatus::Running,
        DocumentTabStatus::NeedsReview,
        DocumentTabStatus::Done,
        DocumentTabStatus::Error,
        DocumentTabStatus::Idle,
    ]) {
        f.status = status;
    }
    let backend = paint(&[project("p", files, true)], None, body(), &theme);
    let tones: Vec<Color> = backend.round_fills.iter().map(|(_, _, c)| *c).collect();
    assert_eq!(
        tones,
        vec![
            theme.primary,
            theme.status_warning,
            theme.status_success,
            theme.destructive
        ],
        "idle has no dot"
    );
}

#[test]
fn a_dirty_file_gets_a_bullet() {
    let theme = Theme::dark();
    let mut projects = shop();
    let clean = paint(&projects, None, body(), &theme);
    projects[0].files[1].dirty = true;
    let dirty = paint(&projects, None, body(), &theme);
    assert_eq!(dirty.round_fills.len(), clean.round_fills.len() + 1);
}

#[test]
fn a_collapsed_project_shows_a_bullet_when_any_file_is_unsaved() {
    let theme = Theme::dark();
    let mut projects = shop();
    projects[1].files[0].dirty = true;
    let backend = paint(&projects, None, body(), &theme);
    assert_eq!(
        backend
            .round_fills
            .iter()
            .filter(|(_, _, color)| *color == theme.foreground)
            .count(),
        1,
        "the collapsed blog project carries the bullet"
    );
}

#[test]
fn collapsed_project_files_are_not_painted() {
    let theme = Theme::dark();
    let backend = paint(&shop(), None, body(), &theme);
    assert!(painted(&backend, "checkout.op"));
    assert!(painted(&backend, "blog"));
    assert!(!painted(&backend, "home.op"), "blog is collapsed");
}

#[test]
fn long_labels_are_ellipsised() {
    let theme = Theme::dark();
    let long = "an-extremely-long-design-file-name-that-cannot-fit-in-the-rail.op";
    let projects = [project("shop", vec![file(long, None)], true)];
    let backend = paint(&projects, None, body(), &theme);
    assert!(!painted(&backend, long), "the full name does not fit");
    assert!(
        backend
            .texts
            .iter()
            .any(|(drawn, _)| drawn.ends_with('…') && drawn.starts_with("an-extremely")),
        "it is cut with an ellipsis: {:?}",
        backend.texts
    );
}

#[test]
fn a_long_project_name_is_ellipsised_and_keeps_the_chip() {
    let theme = Theme::dark();
    let name = "a-project-with-a-name-far-too-long-for-a-two-hundred-pixel-rail";
    let projects = [project(name, vec![file("a.op", None)], true)];
    let backend = paint(&projects, None, body(), &theme);
    assert!(!painted(&backend, name));
    assert!(backend.texts.iter().any(|(drawn, _)| drawn.ends_with('…')));
    assert!(painted(&backend, "1"), "the aggregate chip survives");
}

#[test]
fn the_folder_hint_is_the_parent_directory() {
    assert_eq!(folder_hint("/work/shop").as_deref(), Some("work"));
    assert_eq!(folder_hint("C:\\Users\\me\\shop").as_deref(), Some("me"));
    assert_eq!(folder_hint("/shop"), None);
    assert_eq!(folder_hint(""), None);
}

#[test]
fn an_expanded_project_without_files_shows_the_new_design_row() {
    let theme = Theme::dark();
    let backend = paint(&[project("fresh", Vec::new(), true)], None, body(), &theme);
    assert!(painted(&backend, "New design"));
}

#[test]
fn empty_state_paints_one_muted_line() {
    let theme = Theme::dark();
    let backend = paint(&[], None, body(), &theme);
    assert_eq!(backend.texts.len(), 1);
    assert!(painted(&backend, "No projects yet"));
}

#[test]
fn the_body_is_painted_on_the_rail_tone_with_no_borders() {
    let theme = Theme::light();
    let backend = paint(&shop(), None, body(), &theme);
    assert_eq!(backend.rect_fills[0], (body(), theme.card));
}

#[test]
fn rows_scrolled_out_of_view_paint_nothing() {
    let theme = Theme::dark();
    let projects: Vec<ProjectView> = (0..20)
        .map(|n| project(&format!("p{n}"), vec![file("a.op", None)], true))
        .collect();
    let layout = ProjectsPanelLayout::new(&projects, body(), 0.0);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects: &projects,
        hover: None,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &theme);
    assert!(painted(&backend, "p0"));
    assert!(
        !painted(&backend, "p19"),
        "the last project is below the fold"
    );
}

#[test]
fn an_open_file_is_listed_by_id_not_position() {
    // The row carries the id; reordering rows must not change what it asks for.
    let mut projects = shop();
    projects[0].files.reverse();
    assert_eq!(projects[0].files[1].open_id, Some(DocumentId(7)));
}
