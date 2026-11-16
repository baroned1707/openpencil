//! Proof scenes for the Figma UI3 restyle — the left rail's Projects tab.
//!
//! Seeds `editor_ui.projects_panel` directly (the host scan that normally
//! fills it is a later step), selects the Projects tab, then paints the real
//! editor frame in both themes so `.proof-of-work/90-projects-*.png` shows
//! the tab in context.

use super::super::ui3_proof::{editor_host, render_probes, rgb8};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, LeftPanelTab, ProjectFileView, ProjectRowTarget, ProjectView,
    ThemeMode,
};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::projects_panel_flow as flow;
use op_editor_ui::widgets::projects_panel_layout::RowKind;
use op_editor_ui::Theme;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn file(
    label: &str,
    open: Option<u64>,
    status: DocumentTabStatus,
    dirty: bool,
    active: bool,
) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/designs/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty,
        status,
        active,
    }
}

fn project(name: &str, files: Vec<ProjectFileView>, expanded: bool) -> ProjectView {
    ProjectView {
        root: format!("/Users/me/Work/{name}"),
        name: name.to_string(),
        files,
        expanded,
    }
}

fn theme_of(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::Light => Theme::light(),
    }
}

/// Three projects, mixed files: one Running (the active one), one
/// NeedsReview and dirty, one Error, some not open; the third project is
/// collapsed.
fn mixed_projects() -> Vec<ProjectView> {
    use DocumentTabStatus::*;
    vec![
        project(
            "storefront",
            vec![
                file("checkout.op", Some(1), Running, false, true),
                file("cart.op", Some(2), NeedsReview, true, false),
                file("product-card.op", None, Idle, false, false),
                file("thank-you-page.op", Some(3), Error, false, false),
            ],
            true,
        ),
        project(
            "mobile-app",
            vec![
                file("onboarding.op", Some(4), Done, false, false),
                file("settings.op", None, Idle, false, false),
            ],
            true,
        ),
        project(
            "marketing-site",
            vec![
                file("landing.op", None, NeedsReview, false, false),
                file("pricing.op", None, Idle, false, false),
                file("blog-index.op", None, Idle, false, false),
            ],
            false,
        ),
    ]
}

fn seeded(mode: ThemeMode, projects: Vec<ProjectView>) -> WidgetHostNative {
    let mut host = editor_host(mode, None);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = projects;
    ui.slides_panel.tab = LeftPanelTab::Projects;
    host
}

fn rail_layout(host: &WidgetHostNative) -> op_editor_ui::widgets::ProjectsPanelLayout {
    let state = host.editor_state();
    let rail = canvas_geometry::layer_panel_rect(state, super::super::ui3_proof::PROOF_H);
    flow::layout(state, rail).expect("the Projects tab is on show")
}

/// A pixel just inside the right end of a row, clear of its text.
fn row_probe(host: &WidgetHostNative, kind: RowKind) -> (i32, i32) {
    let rect = rail_layout(host).row(kind).expect("row laid out").rect;
    (
        (rect.origin.x + rect.size.x - 6.0) as i32,
        (rect.origin.y + rect.size.y / 2.0) as i32,
    )
}

fn each_theme(
    name: &str,
    projects: &[ProjectView],
    prepare: impl Fn(&mut WidgetHostNative),
    probe_kinds: &[RowKind],
) -> Vec<(ThemeMode, Vec<[u8; 3]>)> {
    BOTH.iter()
        .map(|&mode| {
            let mut host = seeded(mode, projects.to_vec());
            prepare(&mut host);
            let probes: Vec<(i32, i32)> = probe_kinds
                .iter()
                .map(|kind| row_probe(&host, *kind))
                .collect();
            (mode, render_probes(&mut host, name, &probes))
        })
        .collect()
}

#[test]
fn mixed_projects_with_active_file_proof() {
    let results = each_theme(
        "90-projects-overview",
        &mixed_projects(),
        |_| {},
        &[RowKind::File(0, 0), RowKind::File(0, 1)],
    );
    for (mode, px) in results {
        let theme = theme_of(mode);
        let wash = theme.row_selected_primary;
        let under = rgb8(theme.card);
        let expected: Vec<u8> = [wash.r, wash.g, wash.b]
            .iter()
            .zip(under)
            .map(|(top, below)| {
                (top * 255.0 * wash.a + f32::from(below) * (1.0 - wash.a)).round() as u8
            })
            .collect();
        for (got, want) in px[0].iter().zip(&expected) {
            assert!(
                got.abs_diff(*want) <= 2,
                "{mode:?}: active wash {:?} vs {expected:?}",
                px[0]
            );
        }
        assert_eq!(px[1], rgb8(theme.card), "{mode:?}: others sit on the rail");
    }
}

#[test]
fn hovered_header_and_file_proof() {
    let results = each_theme(
        "90-projects-hover",
        &mixed_projects(),
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::Header(1));
        },
        &[RowKind::Header(1), RowKind::Header(0)],
    );
    for (mode, px) in results {
        let theme = theme_of(mode);
        assert_ne!(px[0], px[1], "{mode:?}: the hovered header is washed");
        assert_eq!(px[1], rgb8(theme.card));
    }
    each_theme(
        "90-projects-hover-file",
        &mixed_projects(),
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::File(0, 2));
        },
        &[],
    );
}

#[test]
fn long_names_truncate_proof() {
    let projects = vec![
        project(
            "a-project-with-a-remarkably-long-name-for-the-rail",
            vec![
                file(
                    "an-extremely-long-design-file-name-for-the-checkout-flow.op",
                    Some(1),
                    DocumentTabStatus::NeedsReview,
                    true,
                    true,
                ),
                file("短い.op", None, DocumentTabStatus::Idle, false, false),
                file(
                    "设计稿-首页-最终版-再次修改-请勿删除.op",
                    Some(2),
                    DocumentTabStatus::Running,
                    false,
                    false,
                ),
            ],
            true,
        ),
        project("x", vec![], true),
    ];
    each_theme("90-projects-long-names", &projects, |_| {}, &[]);
}

#[test]
fn collapsed_project_proof() {
    let mut projects = mixed_projects();
    projects[0].expanded = false;
    projects[1].expanded = false;
    projects[1].files[0].dirty = true;
    each_theme("90-projects-collapsed", &projects, |_| {}, &[]);
}

#[test]
fn new_design_row_for_an_empty_project_proof() {
    let projects = vec![
        project("fresh-start", vec![], true),
        project(
            "storefront",
            vec![file(
                "checkout.op",
                Some(1),
                DocumentTabStatus::Idle,
                false,
                true,
            )],
            true,
        ),
    ];
    each_theme(
        "90-projects-new-design",
        &projects,
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::NewDesignRow(0));
        },
        &[],
    );
}

#[test]
fn scrolled_long_list_proof() {
    let projects: Vec<ProjectView> = (1..=9)
        .map(|n| {
            project(
                &format!("project-{n:02}"),
                vec![
                    file("home.op", None, DocumentTabStatus::Idle, false, false),
                    file(
                        "settings.op",
                        Some(n),
                        DocumentTabStatus::Done,
                        false,
                        false,
                    ),
                    file("pricing.op", None, DocumentTabStatus::Idle, false, false),
                ],
                true,
            )
        })
        .collect();
    each_theme(
        "90-projects-scrolled",
        &projects,
        |host| {
            host.editor_state_mut()
                .editor_ui
                .projects_panel
                .scroll
                .offset = 170.0;
        },
        &[],
    );
    // The same scene must actually overflow, or the scroll proof proves nothing.
    let host = seeded(ThemeMode::Dark, projects);
    assert!(rail_layout(&host).max_scroll() > 170.0);
}

/// The empty body, rendered on its own: the host never selects the tab
/// without projects (it falls back to Layers), so the widget is painted
/// directly onto a rail-sized surface.
#[test]
fn empty_state_proof() {
    use op_editor_ui::widgets::projects_panel_layout::ProjectsPanelLayout;
    use op_editor_ui::widgets::{PaintCx, ProjectsPanel, ProjectsPanelText};
    for mode in BOTH {
        let theme = theme_of(mode);
        let text = ProjectsPanelText {
            empty: op_editor_ui::i18n::translate(op_editor_core::Locale::EnUs, "projects.empty"),
            new_design: "New design",
            needs_review: "{{count}} need review",
            running: "{{count}} running",
        };
        let panel = op_editor_ui::Rect::xywh(0.0, 0.0, 280.0, 220.0);
        let layout = ProjectsPanelLayout::new(&[], panel, 0.0);
        let mut backend = crate::backend::NativeBackend::with_dpi(1.0);
        let mut surface = skia_safe::surfaces::raster_n32_premul((280, 220)).unwrap();
        for _ in 0..2 {
            let mut frame = crate::backend::NativeFrameBackend::new(&mut backend, surface.canvas());
            let mut cx = PaintCx {
                backend: &mut frame,
            };
            ProjectsPanel {
                projects: &[],
                hover: None,
                text: &text,
            }
            .paint(&mut cx, &layout, &theme);
        }
        let corner = surface.peek_pixels().unwrap().get_color((2, 2));
        assert_eq!([corner.r(), corner.g(), corner.b()], rgb8(theme.card));
        if let Ok(dir) = std::env::var("OP_PROOF_DIR") {
            let suffix = if mode == ThemeMode::Dark {
                "dark"
            } else {
                "light"
            };
            let bytes = surface
                .image_snapshot()
                .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
                .expect("png encodes");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("90-projects-empty-{suffix}.png")),
                bytes.as_bytes(),
            )
            .unwrap();
        }
    }
}
