//! The Projects list's `.fig` rows and "Convert all" rows: where they sit, what
//! a press on them asks the host for, and what they paint.

use crate::theme::Theme;
use crate::widgets::projects_panel::ProjectsPanel;
use crate::widgets::projects_panel_flow::{self, projects_panel_flow_tests::*, ProjectsPress};
use crate::widgets::projects_panel_layout::{ProjectsPanelLayout, RowKind, FILE_HEIGHT};
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::PaintCx;
use crate::Point2D;
use op_editor_core::{
    FigStatus, ProjectFileKind, ProjectFileView, ProjectRequest, ProjectRowTarget, ProjectView,
};

fn fig(label: &str, folder: &str, status: FigStatus) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/repo/{folder}/{label}"),
        folder: folder.to_string(),
        kind: ProjectFileKind::Figma(status),
        ..file(label, None)
    }
}

fn design(label: &str, folder: &str) -> ProjectFileView {
    ProjectFileView {
        folder: folder.to_string(),
        ..file(label, None)
    }
}

fn repo(files: Vec<ProjectFileView>) -> ProjectView {
    let mut view = project("repo", files, true);
    view.closable = true;
    view
}

fn kinds(layout: &ProjectsPanelLayout) -> Vec<RowKind> {
    layout.rows.iter().map(|row| row.kind).collect()
}

fn paint(projects: &[ProjectView], hover: Option<ProjectRowTarget>) -> CaptureBackend {
    let layout = ProjectsPanelLayout::new(projects, BODY, 0.0);
    let mut backend = CaptureBackend::default();
    ProjectsPanel {
        projects,
        tabs: &[],
        hover,
        rename: None,
        trash_confirm: None,
        now_ms: 0,
        text: &TEXT,
    }
    .paint(
        &mut PaintCx {
            backend: &mut backend,
        },
        &layout,
        &Theme::dark(),
    );
    backend
}

fn painted(backend: &CaptureBackend, text: &str) -> bool {
    backend.texts.iter().any(|(drawn, _)| drawn == text)
}

#[test]
fn a_project_with_figs_to_convert_offers_convert_all_under_its_header() {
    let view = repo(vec![
        design("home.op", ""),
        fig("a.fig", "", FigStatus::NotConverted),
        fig("b.fig", "", FigStatus::NotConverted),
        fig("done.fig", "", FigStatus::Converted),
    ]);
    assert_eq!(view.not_converted_count(None), 2);
    let layout = ProjectsPanelLayout::new(&[view], BODY, 0.0);
    assert_eq!(
        kinds(&layout),
        [
            RowKind::Header(0),
            RowKind::ConvertAll(0, None),
            RowKind::File(0, 0),
            RowKind::File(0, 1),
            RowKind::File(0, 2),
            RowKind::File(0, 3),
        ]
    );
    let row = layout.row(RowKind::ConvertAll(0, None)).unwrap();
    assert_eq!(row.rect.size.y, FILE_HEIGHT);
    let centre = Point2D::new(
        row.rect.origin.x + 40.0,
        row.rect.origin.y + row.rect.size.y / 2.0,
    );
    assert_eq!(
        layout.hit(centre),
        Some(ProjectRowTarget::ConvertAll(0, None))
    );
}

#[test]
fn nothing_to_convert_means_no_convert_all_row() {
    let view = repo(vec![
        design("home.op", ""),
        fig("done.fig", "", FigStatus::Converted),
        fig("busy.fig", "", FigStatus::Converting),
        fig("bad.fig", "", FigStatus::Failed("timed out".into())),
    ]);
    let layout = ProjectsPanelLayout::new(&[view], BODY, 0.0);
    assert!(!kinds(&layout)
        .iter()
        .any(|kind| matches!(kind, RowKind::ConvertAll(..))));
}

#[test]
fn a_folder_with_figs_to_convert_has_its_own_convert_all_row() {
    let view = repo(vec![
        fig("a.fig", "", FigStatus::NotConverted),
        fig("b.fig", "screens", FigStatus::NotConverted),
        fig("c.fig", "screens", FigStatus::NotConverted),
        design("d.op", "other"),
    ]);
    let layout = ProjectsPanelLayout::new(std::slice::from_ref(&view), BODY, 0.0);
    assert_eq!(
        kinds(&layout),
        [
            RowKind::Header(0),
            RowKind::ConvertAll(0, None),
            RowKind::File(0, 0),
            RowKind::Folder(0, 1),
            RowKind::ConvertAll(0, Some(1)),
            RowKind::File(0, 1),
            RowKind::File(0, 2),
            RowKind::Folder(0, 3),
            RowKind::File(0, 3),
        ],
        "the folder with nothing to convert has no row"
    );
    assert_eq!(view.not_converted_count(Some("screens")), 2);

    // A press on the folder's row asks for exactly that folder.
    let mut state = state_with(vec![view]);
    let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, BODY, 0.0);
    let rect = layout.row(RowKind::ConvertAll(0, Some(1))).unwrap().rect;
    let at = Point2D::new(rect.origin.x + 40.0, rect.origin.y + rect.size.y / 2.0);
    assert_eq!(
        projects_panel_flow::press(&mut state, &layout, at, 0),
        ProjectsPress::Claimed(Some(ProjectRowTarget::ConvertAll(0, Some(1))))
    );
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ConvertAllFigs(
            "/work/repo".to_string(),
            Some("screens".to_string())
        ))
    );
}

#[test]
fn one_folder_holding_every_fig_is_offered_once_not_twice() {
    let view = repo(vec![
        design("home.op", ""),
        fig("b.fig", "screens", FigStatus::NotConverted),
    ]);
    let layout = ProjectsPanelLayout::new(&[view], BODY, 0.0);
    let converts: Vec<RowKind> = kinds(&layout)
        .into_iter()
        .filter(|kind| matches!(kind, RowKind::ConvertAll(..)))
        .collect();
    assert_eq!(converts, [RowKind::ConvertAll(0, Some(1))]);
}

#[test]
fn a_folded_folder_hands_its_figs_to_the_project_row() {
    let mut view = repo(vec![fig("b.fig", "screens", FigStatus::NotConverted)]);
    view.collapsed_folders = vec!["screens".to_string()];
    let layout = ProjectsPanelLayout::new(&[view], BODY, 0.0);
    assert_eq!(
        kinds(&layout),
        [
            RowKind::Header(0),
            RowKind::ConvertAll(0, None),
            RowKind::Folder(0, 0)
        ]
    );
}

#[test]
fn pressing_a_fig_row_never_opens_it_as_a_document() {
    let statuses = [
        FigStatus::NotConverted,
        FigStatus::Converted,
        FigStatus::Converting,
        FigStatus::Failed("too large".into()),
    ];
    for status in statuses {
        let mut state = state_with(vec![repo(vec![fig("a.fig", "", status.clone())])]);
        let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, BODY, 0.0);
        let rect = layout.row(RowKind::File(0, 0)).unwrap().rect;
        let at = Point2D::new(rect.origin.x + 40.0, rect.origin.y + rect.size.y / 2.0);
        projects_panel_flow::press(&mut state, &layout, at, 0);
        assert_eq!(
            state.editor_ui.projects_panel.pending_request,
            Some(ProjectRequest::ConvertFig("/work/repo//a.fig".to_string())),
            "{status:?}: the host decides between converting and opening the .op"
        );
    }
}

#[test]
fn pressing_convert_all_for_the_project_asks_for_the_whole_project() {
    let mut state = state_with(vec![repo(vec![fig("a.fig", "", FigStatus::NotConverted)])]);
    let layout = ProjectsPanelLayout::new(&state.editor_ui.projects_panel.projects, BODY, 0.0);
    let rect = layout.row(RowKind::ConvertAll(0, None)).unwrap().rect;
    let at = Point2D::new(rect.origin.x + 40.0, rect.origin.y + rect.size.y / 2.0);
    projects_panel_flow::press(&mut state, &layout, at, 0);
    assert_eq!(
        state.editor_ui.projects_panel.pending_request,
        Some(ProjectRequest::ConvertAllFigs(
            "/work/repo".to_string(),
            None
        ))
    );
}

#[test]
fn a_fig_row_paints_its_tag_and_a_status_word_per_state() {
    let cases = [
        (FigStatus::NotConverted, "Convert"),
        (FigStatus::Converted, "Converted"),
        (FigStatus::Converting, "Converting…"),
        (FigStatus::Failed("timed out".into()), "Failed"),
    ];
    let theme = Theme::dark();
    for (status, word) in cases {
        let backend = paint(&[repo(vec![fig("a.fig", "", status.clone())])], None);
        assert!(painted(&backend, "FIG"), "{status:?}: the kind tag");
        assert!(painted(&backend, "a.fig"), "{status:?}: the name");
        assert!(
            painted(&backend, word),
            "{status:?}: its status word `{word}`"
        );
        assert!(
            backend
                .rect_fills
                .iter()
                .any(|(_, color)| *color == theme.button_hover),
            "{status:?}: the tag is a flat fill"
        );
    }
}

#[test]
fn convert_all_paints_its_count_and_a_design_row_paints_no_tag() {
    let projects = [repo(vec![
        design("home.op", ""),
        fig("a.fig", "", FigStatus::NotConverted),
        fig("b.fig", "", FigStatus::NotConverted),
        fig("c.fig", "", FigStatus::NotConverted),
    ])];
    let backend = paint(&projects, None);
    assert!(painted(&backend, "Convert all (3)"));
    let only_designs = [repo(vec![design("home.op", "")])];
    let backend = paint(&only_designs, None);
    assert!(!painted(&backend, "FIG"));
    assert!(!painted(&backend, "Convert all (0)"));
}

#[test]
fn hovering_a_fig_or_convert_all_row_washes_it_and_shows_a_tooltip() {
    let theme = Theme::dark();
    let projects = [repo(vec![
        fig("a.fig", "", FigStatus::Failed("timed out".into())),
        fig("b.fig", "", FigStatus::NotConverted),
    ])];
    let accent = |backend: &CaptureBackend| {
        backend
            .rect_fills
            .iter()
            .filter(|(_, color)| *color == theme.accent)
            .count()
    };
    assert_eq!(accent(&paint(&projects, None)), 0);
    // Rows: header, convert-all, a.fig, b.fig.
    let backend = paint(&projects, Some(ProjectRowTarget::File(0, 0)));
    assert_eq!(accent(&backend), 1);
    assert!(
        painted(&backend, "Conversion failed: timed out. Click to retry."),
        "a failed row's tooltip carries the reason"
    );
    let backend = paint(&projects, Some(ProjectRowTarget::ConvertAll(0, None)));
    assert_eq!(accent(&backend), 1);
    assert!(painted(
        &backend,
        "Convert every .fig file here to .op next to it"
    ));
    let backend = paint(&projects, Some(ProjectRowTarget::File(0, 1)));
    assert!(painted(&backend, "Convert to .op next to this file"));
}

#[test]
fn a_long_fig_name_is_ellipsized_before_its_status_word() {
    let long = "an-extremely-long-figma-export-file-name-for-the-checkout-flow.fig";
    let layout_rect = ProjectsPanelLayout::new(
        &[repo(vec![fig(long, "", FigStatus::Converted)])],
        BODY,
        0.0,
    )
    .row(RowKind::File(0, 0))
    .unwrap()
    .rect;
    let backend = paint(&[repo(vec![fig(long, "", FigStatus::Converted)])], None);
    assert!(painted(&backend, "Converted"), "the status stays whole");
    let name = backend
        .texts
        .iter()
        .find(|(text, _)| text.ends_with('…') && text.starts_with("an-extremely"))
        .expect("the name is cut with an ellipsis");
    assert!(name.1.x < layout_rect.origin.x + layout_rect.size.x);
    let status = backend
        .texts
        .iter()
        .find(|(text, _)| text == "Converted")
        .unwrap();
    assert!(
        status.1.x > name.1.x,
        "the name does not run under the status word"
    );
}
