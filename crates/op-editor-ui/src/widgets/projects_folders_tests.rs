//! The Projects dock's folder grouping, status lines, header buttons and the
//! "Open project" targets: layout, hit-testing, what a press records and what
//! reaches the canvas.

use crate::theme::Theme;
use crate::widgets::projects_dock::ProjectsDock;
use crate::widgets::projects_dock_flow::{self as dock_flow, DockPress};
use crate::widgets::projects_dock_layout::{DockHit, ProjectsDockLayout, STRIP_TARGET_HEIGHT};
use crate::widgets::projects_panel::{ProjectsPanel, ProjectsPanelText};
use crate::widgets::projects_panel_flow::{self, projects_panel_flow_tests::*, ProjectsPress};
use crate::widgets::projects_panel_layout::{ProjectsPanelLayout, RowKind};
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};
use op_editor_core::{ProjectRequest, ProjectRowTarget, ProjectScanState, ProjectView};

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

fn in_folder(label: &str, folder: &str) -> op_editor_core::ProjectFileView {
    let mut view = file(label, None);
    view.folder = folder.to_string();
    view
}

/// Files in three nested folders plus the root.
fn repo(collapsed: &[&str]) -> ProjectView {
    let mut view = project(
        "repo",
        vec![
            in_folder("root.op", ""),
            in_folder("home.op", "designs"),
            in_folder("cart.op", "screens"),
            in_folder("login.op", "screens/auth"),
            in_folder("signup.op", "screens/auth"),
        ],
        true,
    );
    view.closable = true;
    view.collapsed_folders = collapsed.iter().map(|name| name.to_string()).collect();
    view
}

fn kinds(layout: &ProjectsPanelLayout) -> Vec<RowKind> {
    layout.rows.iter().map(|row| row.kind).collect()
}

fn body() -> Rect {
    BODY
}

#[test]
fn files_group_under_a_sub_header_per_folder_and_root_files_have_none() {
    let layout = ProjectsPanelLayout::new(&[repo(&[])], body(), 0.0);
    assert_eq!(
        kinds(&layout),
        [
            RowKind::Header(0),
            RowKind::File(0, 0),
            RowKind::Folder(0, 1),
            RowKind::File(0, 1),
            RowKind::Folder(0, 2),
            RowKind::File(0, 2),
            RowKind::Folder(0, 3),
            RowKind::File(0, 3),
            RowKind::File(0, 4),
        ]
    );
}

#[test]
fn a_folded_folder_keeps_its_header_and_hides_only_its_own_files() {
    let layout = ProjectsPanelLayout::new(&[repo(&["screens/auth"])], body(), 0.0);
    let rows = kinds(&layout);
    assert!(rows.contains(&RowKind::Folder(0, 3)), "the header stays");
    assert!(!rows.contains(&RowKind::File(0, 3)));
    assert!(!rows.contains(&RowKind::File(0, 4)));
    assert!(
        rows.contains(&RowKind::File(0, 2)),
        "a sibling folder is unaffected"
    );
    // Folding the project itself hides everything under its header.
    let mut folded = repo(&[]);
    folded.expanded = false;
    let layout = ProjectsPanelLayout::new(&[folded], body(), 0.0);
    assert_eq!(kinds(&layout), [RowKind::Header(0)]);
}

#[test]
fn pressing_a_folder_header_asks_the_host_to_fold_it() {
    let mut state = state_with(vec![repo(&[])]);
    let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, body(), 0.0);
    let rect = layout.row(RowKind::Folder(0, 3)).unwrap().rect;
    let at = Point2D::new(rect.origin.x + 40.0, rect.origin.y + rect.size.y / 2.0);
    projects_panel_flow::cursor_move(&mut state, &layout, at);
    assert_eq!(
        projects_panel_flow::press(&mut state, &layout, at, 0),
        ProjectsPress::Claimed(Some(ProjectRowTarget::Folder(0, 3)))
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ToggleFolder(
            "/work/repo".into(),
            "screens/auth".into()
        ))
    );
}

#[test]
fn only_an_opened_project_has_close_and_refresh_and_refresh_only_while_idle() {
    let inferred = project("shop", vec![file("a.op", None)], true);
    let mut opened = repo(&[]);
    let mut scanning = repo(&[]);
    scanning.scan = ProjectScanState::Scanning;
    let layout = ProjectsPanelLayout::new(&[inferred, opened.clone(), scanning], body(), 0.0);
    let header = |index| layout.row(RowKind::Header(index)).unwrap();
    assert!(header(0).close.is_none() && header(0).refresh.is_none());
    assert!(header(0).plus.is_some(), "`+` stays on every project");
    assert!(header(1).close.is_some() && header(1).refresh.is_some());
    assert!(header(2).close.is_some());
    assert!(header(2).refresh.is_none(), "no refresh while a scan runs");
    // Left to right: refresh, close, `+`, none overlapping.
    let (refresh, close, plus) = (
        header(1).refresh.unwrap(),
        header(1).close.unwrap(),
        header(1).plus.unwrap(),
    );
    assert!(refresh.origin.x + refresh.size.x <= close.origin.x);
    assert!(close.origin.x + close.size.x <= plus.origin.x);
    opened.closable = false;
    let layout = ProjectsPanelLayout::new(&[opened], body(), 0.0);
    assert!(layout.row(RowKind::Header(0)).unwrap().close.is_none());
}

#[test]
fn close_and_refresh_buttons_hit_test_and_record_their_requests() {
    let mut state = state_with(vec![repo(&[])]);
    let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, body(), 0.0);
    let header = layout.row(RowKind::Header(0)).unwrap();
    let close = header.close.unwrap();
    let refresh = header.refresh.unwrap();
    let centre = |rect: Rect| {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    };
    assert_eq!(layout.hit(centre(close)), Some(ProjectRowTarget::Close(0)));
    assert_eq!(
        layout.hit(centre(refresh)),
        Some(ProjectRowTarget::Refresh(0))
    );

    projects_panel_flow::cursor_move(&mut state, &layout, centre(close));
    projects_panel_flow::press(&mut state, &layout, centre(close), 0);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::CloseProject("/work/repo".into()))
    );
    projects_panel_flow::cursor_move(&mut state, &layout, centre(refresh));
    projects_panel_flow::press(&mut state, &layout, centre(refresh), 0);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::RefreshProject("/work/repo".into()))
    );
}

#[test]
fn a_press_that_never_saw_the_header_hovered_is_a_header_press_not_a_close() {
    let mut state = state_with(vec![repo(&[])]);
    let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, body(), 0.0);
    let close = layout.row(RowKind::Header(0)).unwrap().close.unwrap();
    let at = Point2D::new(close.origin.x + 5.0, close.origin.y + 5.0);
    // No hover first: a synthesized click must not close a project.
    projects_panel_flow::press(&mut state, &layout, at, 0);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ToggleProject("/work/repo".into()))
    );
}

#[test]
fn status_rows_follow_the_scan_state() {
    let with = |scan, files: Vec<_>| {
        let mut view = project("p", files, true);
        view.scan = scan;
        ProjectsPanelLayout::new(&[view], body(), 0.0)
    };
    let one = || vec![file("a.op", None)];
    assert_eq!(
        kinds(&with(ProjectScanState::Scanning, one())),
        [RowKind::Header(0), RowKind::Status(0), RowKind::File(0, 0)]
    );
    assert_eq!(
        kinds(&with(ProjectScanState::Truncated(7), one())),
        [RowKind::Header(0), RowKind::File(0, 0), RowKind::Status(0)],
        "`N more not shown` closes the list"
    );
    assert_eq!(
        kinds(&with(ProjectScanState::Idle, Vec::new())),
        [
            RowKind::Header(0),
            RowKind::NoDesigns(0),
            RowKind::NewDesign(0)
        ],
        "an empty folder says so and offers New design"
    );
    assert_eq!(
        kinds(&with(ProjectScanState::Scanning, Vec::new())),
        [
            RowKind::Header(0),
            RowKind::Status(0),
            RowKind::NewDesign(0)
        ],
        "no 'nothing found' while still looking"
    );
    assert_eq!(
        kinds(&with(ProjectScanState::Idle, one())),
        [RowKind::Header(0), RowKind::File(0, 0)]
    );
}

fn paint_list(projects: &[ProjectView], hover: Option<ProjectRowTarget>) -> CaptureBackend {
    let layout = ProjectsPanelLayout::new(projects, body(), 0.0);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    ProjectsPanel {
        projects,
        hover,
        rename: None,
        now_ms: 0,
        text: &TEXT,
    }
    .paint(&mut cx, &layout, &Theme::dark());
    backend
}

fn said(backend: &CaptureBackend, text: &str) -> bool {
    backend.texts.iter().any(|(drawn, _)| drawn == text)
}

#[test]
fn folder_headers_and_status_lines_are_painted_with_their_text() {
    let mut view = repo(&["screens"]);
    view.scan = ProjectScanState::Truncated(12);
    let backend = paint_list(&[view], None);
    assert!(said(&backend, "designs/"));
    assert!(said(&backend, "screens/"));
    assert!(said(&backend, "screens/auth/"));
    assert!(said(&backend, "12 more not shown"));
    assert!(
        said(&backend, "1"),
        "a folded folder shows how many files it hides"
    );
    let mut slow = repo(&[]);
    slow.scan = ProjectScanState::Slow;
    assert!(said(
        &paint_list(&[slow], None),
        "Slow folder: showing what was found"
    ));
    let mut scanning = repo(&[]);
    scanning.scan = ProjectScanState::Scanning;
    assert!(said(&paint_list(&[scanning], None), "Scanning…"));
}

#[test]
fn a_long_folder_path_keeps_its_tail() {
    let mut view = project(
        "p",
        vec![in_folder(
            "a.op",
            "very/long/folder/path/that/never/fits/in/a/dock/screens/auth",
        )],
        true,
    );
    view.closable = true;
    let backend = paint_list(&[view], None);
    let label = backend
        .texts
        .iter()
        .map(|(text, _)| text.as_str())
        .find(|text| text.ends_with("auth/"))
        .expect("the nearest folder stays readable");
    assert!(label.starts_with('…'), "{label}");
}

#[test]
fn hovering_a_header_button_paints_its_tooltip() {
    let view = repo(&[]);
    let backend = paint_list(
        std::slice::from_ref(&view),
        Some(ProjectRowTarget::Close(0)),
    );
    assert!(said(&backend, "Close project"));
    let backend = paint_list(
        std::slice::from_ref(&view),
        Some(ProjectRowTarget::Refresh(0)),
    );
    assert!(said(&backend, "Refresh"));
    let quiet = paint_list(&[view], Some(ProjectRowTarget::Header(0)));
    assert!(!said(&quiet, "Close project"));
}

#[test]
fn the_empty_list_has_one_open_project_button_that_requests_the_picker() {
    let mut state = state_with(Vec::new());
    let layout = ProjectsPanelLayout::new(&[], body(), 0.0);
    let button = layout.empty_button.expect("the empty list has a button");
    let at = Point2D::new(button.origin.x + 10.0, button.origin.y + 10.0);
    assert_eq!(layout.hit(at), Some(ProjectRowTarget::OpenProject));
    projects_panel_flow::press(&mut state, &layout, at, 0);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenProjectPicker)
    );
    assert!(ProjectsPanelLayout::new(&[repo(&[])], body(), 0.0)
        .empty_button
        .is_none());
}

fn dock_rect(open: bool) -> Rect {
    Rect::xywh(1_168.0, 40.0, if open { 232.0 } else { 28.0 }, 760.0)
}

#[test]
fn the_empty_strip_stacks_a_fold_chevron_over_an_open_project_button() {
    let layout = ProjectsDockLayout::new(&[], dock_rect(false), false, 0.0);
    let open = layout.open_project.expect("the empty strip offers it");
    assert_eq!(layout.toggle.size.y, STRIP_TARGET_HEIGHT);
    assert!(open.origin.y >= layout.toggle.origin.y + layout.toggle.size.y);
    let centre = |rect: Rect| {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    };
    assert_eq!(layout.hit(centre(open)), Some(DockHit::OpenProject));
    assert_eq!(layout.hit(centre(layout.toggle)), Some(DockHit::Toggle));

    // A strip with projects is one big target that folds the dock out.
    let with_projects = ProjectsDockLayout::new(&[repo(&[])], dock_rect(false), false, 0.0);
    assert!(with_projects.open_project.is_none());
    assert_eq!(with_projects.toggle, dock_rect(false));
}

#[test]
fn clicking_the_empty_strips_folder_plus_opens_the_picker_and_its_chevron_unfolds() {
    let mut state = state_with(Vec::new());
    let layout = dock_flow::layout(&state, 1_400.0, 800.0).expect("an empty dock is a strip");
    assert!(!layout.open);
    let open = layout.open_project.unwrap();
    let at = Point2D::new(open.origin.x + 5.0, open.origin.y + 5.0);
    dock_flow::cursor_move(&mut state, &layout, at);
    assert!(state.editor_ui.projects_panel.open_project_hover);
    assert_eq!(
        dock_flow::press(&mut state, &layout, at, 0),
        DockPress::Claimed
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenProjectPicker)
    );
    assert!(
        !state.editor_ui.projects_panel.is_open(),
        "the picker does not unfold it"
    );

    let chevron = Point2D::new(layout.toggle.origin.x + 5.0, layout.toggle.origin.y + 5.0);
    dock_flow::press(&mut state, &layout, chevron, 0);
    assert!(
        state.editor_ui.projects_panel.is_open(),
        "the chevron unfolds the empty dock"
    );
    assert!(
        state.editor_ui.projects_panel.dock_open,
        "the choice with projects is untouched"
    );
    assert!(
        !state.editor_ui.projects_panel.open_project_hover,
        "folding drops the hover of the layout it replaced"
    );
}

#[test]
fn the_open_dock_header_has_an_open_project_button_beside_the_chevron() {
    let mut state = state_with(vec![repo(&[])]);
    let layout = dock_flow::layout(&state, 1_400.0, 800.0).unwrap();
    let open = layout.open_project.expect("the open header has the button");
    assert!(
        open.origin.x + open.size.x <= layout.toggle.origin.x,
        "left of the chevron"
    );
    let at = Point2D::new(open.origin.x + 4.0, open.origin.y + 4.0);
    assert_eq!(layout.hit(at), Some(DockHit::OpenProject));
    assert_eq!(
        dock_flow::press(&mut state, &layout, at, 0),
        DockPress::Claimed
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::OpenProjectPicker)
    );
    assert!(state.editor_ui.projects_panel.dock_open, "still open");
}

#[test]
fn the_dock_paints_the_folder_plus_glyph_where_it_offers_open_project() {
    let paint = |projects: &[ProjectView], open: bool| {
        let layout = ProjectsDockLayout::new(projects, dock_rect(open), open, 0.0);
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
            toggle_hover: false,
            open_project_hover: false,
            has_projects: !projects.is_empty(),
        }
        .paint(&mut cx, &layout, &Theme::dark());
        backend
    };
    let plus = crate::widgets::icons::Icon::FolderPlus.paths();
    let drew_plus = |backend: &CaptureBackend| {
        backend
            .svg_strokes
            .iter()
            .any(|(path, ..)| plus.contains(&path.as_str()))
    };
    assert!(drew_plus(&paint(&[], false)), "the empty strip");
    assert!(
        drew_plus(&paint(&[], true)),
        "the empty dock's header and button"
    );
    assert!(drew_plus(&paint(&[repo(&[])], true)), "the open header");
    assert!(
        !drew_plus(&paint(&[repo(&[])], false)),
        "a strip with projects shows the count"
    );
}
