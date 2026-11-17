//! Proof scenes for the Figma UI3 restyle — the right-edge Projects dock.
//!
//! Seeds `editor_ui.projects_panel` directly (the host scan that normally
//! fills it is a later step) on a host that supports document tabs, then
//! paints the real editor frame in both themes so
//! `.proof-of-work/90-dock-*.png` shows the dock in context: beside the
//! Design panel, collapsed to its strip, hovered, scrolled.

use super::super::ui3_proof::{editor_host, render_probes, rgb8, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, ProjectFileView, ProjectRowTarget, ProjectView, ThemeMode,
};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::projects_dock_flow as dock_flow;
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
        folder: String::new(),
        kind: op_editor_core::ProjectFileKind::Design,
    }
}

fn project(name: &str, files: Vec<ProjectFileView>, expanded: bool) -> ProjectView {
    ProjectView {
        root: format!("/Users/me/Work/{name}"),
        name: name.to_string(),
        files,
        expanded,
        ..ProjectView::default()
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

fn seeded(mode: ThemeMode, projects: Vec<ProjectView>, selected: Option<&str>) -> WidgetHostNative {
    let mut host = editor_host(mode, selected);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = projects;
    // The dock narrowed the canvas: frame the artboard in what is left.
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

fn dock_layout(host: &WidgetHostNative) -> op_editor_ui::widgets::ProjectsDockLayout {
    dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H).expect("the dock is shown")
}

/// A pixel just inside the right end of a list row, clear of its text.
fn row_probe(host: &WidgetHostNative, kind: RowKind) -> (i32, i32) {
    let layout = dock_layout(host);
    let rect = layout
        .list
        .as_ref()
        .and_then(|list| list.row(kind))
        .expect("row laid out")
        .rect;
    (
        (rect.origin.x + rect.size.x - 6.0) as i32,
        (rect.origin.y + rect.size.y / 2.0) as i32,
    )
}

/// The centre of the canvas region: where the artboard must sit.
fn canvas_centre(host: &WidgetHostNative) -> (i32, i32) {
    let canvas = canvas_geometry::canvas_rect(host.editor_state(), PROOF_W, PROOF_H);
    (
        (canvas.origin.x + canvas.size.x / 2.0) as i32,
        (canvas.origin.y + canvas.size.y / 2.0) as i32,
    )
}

fn each_theme(
    name: &str,
    projects: &[ProjectView],
    selected: Option<&str>,
    prepare: impl Fn(&mut WidgetHostNative),
    probe_kinds: &[RowKind],
) -> Vec<(ThemeMode, Vec<[u8; 3]>)> {
    BOTH.iter()
        .map(|&mode| {
            let mut host = seeded(mode, projects.to_vec(), selected);
            prepare(&mut host);
            let mut probes: Vec<(i32, i32)> = probe_kinds
                .iter()
                .map(|kind| row_probe(&host, *kind))
                .collect();
            probes.push(canvas_centre(&host));
            let mut pixels = render_probes(&mut host, name, &probes);
            // The artboard (white) sits at the centre of the NARROWED canvas.
            let centre = pixels.pop().expect("canvas probe");
            assert_eq!(centre, [255, 255, 255], "{mode:?}: artboard is centred");
            (mode, pixels)
        })
        .collect()
}

#[test]
fn dock_open_with_no_selection_proof() {
    let results = each_theme(
        "90-dock-open",
        &mixed_projects(),
        None,
        |_| {},
        &[RowKind::File(0, 0), RowKind::File(0, 1)],
    );
    for (mode, px) in results {
        let theme = theme_of(mode);
        let wash = theme.row_selected_primary;
        let under = rgb8(theme.muted);
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
        assert_eq!(px[1], rgb8(theme.muted), "{mode:?}: others sit on the dock");
    }
}

#[test]
fn dock_beside_the_design_panel_proof() {
    for mode in BOTH {
        let mut host = seeded(mode, mixed_projects(), Some("card"));
        let property = canvas_geometry::property_panel_rect(host.editor_state(), PROOF_W, PROOF_H);
        let dock = dock_layout(&host).rect;
        assert_eq!(property.origin.x + property.size.x, dock.origin.x);
        // One pixel inside each column, at the same height.
        let y = (PROOF_H - 24.0) as i32;
        let probes = [
            ((property.origin.x + 6.0) as i32, y),
            ((dock.origin.x + 6.0) as i32, y),
            canvas_centre(&host),
        ];
        let px = render_probes(&mut host, "90-dock-with-design-panel", &probes);
        let theme = theme_of(mode);
        assert_eq!(px[0], rgb8(theme.card), "{mode:?}: Design panel tone");
        assert_eq!(px[1], rgb8(theme.muted), "{mode:?}: the dock's own tone");
        assert_ne!(px[0], px[1], "{mode:?}: the dock is tone-separated from it");
    }
}

#[test]
fn collapsed_dock_strip_proof() {
    for mode in BOTH {
        let mut host = seeded(mode, mixed_projects(), None);
        let open_canvas = canvas_geometry::canvas_rect(host.editor_state(), PROOF_W, PROOF_H);
        host.editor_state_mut().editor_ui.projects_panel.dock_open = false;
        host.mark_dirty();
        host.zoom_to_fit(PROOF_W, PROOF_H);
        let canvas = canvas_geometry::canvas_rect(host.editor_state(), PROOF_W, PROOF_H);
        assert!(canvas.size.x > open_canvas.size.x);
        let centre = canvas_centre(&host);
        let px = render_probes(&mut host, "90-dock-collapsed", &[centre]);
        assert_eq!(px[0], [255, 255, 255], "{mode:?}: artboard centred");
    }
}

#[test]
fn collapsed_dock_hover_proof() {
    for mode in BOTH {
        let mut host = seeded(mode, mixed_projects(), Some("card"));
        host.editor_state_mut().editor_ui.projects_panel.dock_open = false;
        host.editor_state_mut()
            .editor_ui
            .projects_panel
            .toggle_hover = true;
        host.mark_dirty();
        host.zoom_to_fit(PROOF_W, PROOF_H);
        render_probes(&mut host, "90-dock-collapsed-hover", &[(0, 0)]);
    }
}

#[test]
fn hovered_header_and_file_proof() {
    let results = each_theme(
        "90-dock-hover-header",
        &mixed_projects(),
        None,
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::Header(1));
        },
        &[RowKind::Header(1), RowKind::Header(0)],
    );
    for (mode, px) in results {
        let theme = theme_of(mode);
        assert_ne!(px[0], px[1], "{mode:?}: the hovered header is washed");
        assert_eq!(px[1], rgb8(theme.muted));
    }
    each_theme(
        "90-dock-hover-file",
        &mixed_projects(),
        None,
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::File(0, 2));
        },
        &[],
    );
    each_theme(
        "90-dock-hover-chevron",
        &mixed_projects(),
        None,
        |host| {
            host.editor_state_mut()
                .editor_ui
                .projects_panel
                .toggle_hover = true;
        },
        &[],
    );
}

#[test]
fn long_names_truncate_proof() {
    let projects = vec![
        project(
            "a-project-with-a-remarkably-long-name-for-the-dock",
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
    each_theme("90-dock-long-names", &projects, Some("card"), |_| {}, &[]);
}

#[test]
fn collapsed_project_proof() {
    let mut projects = mixed_projects();
    projects[0].expanded = false;
    projects[1].expanded = false;
    projects[1].files[0].dirty = true;
    each_theme("90-dock-projects-collapsed", &projects, None, |_| {}, &[]);
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
        "90-dock-new-design",
        &projects,
        None,
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::NewDesignRow(0));
        },
        &[],
    );
}

#[test]
fn scrolled_long_list_proof() {
    let projects: Vec<ProjectView> = (1..=12)
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
        "90-dock-scrolled",
        &projects,
        None,
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
    let host = seeded(ThemeMode::Dark, projects, None);
    let list = dock_layout(&host).list.expect("open dock has a list");
    assert!(list.max_scroll() > 170.0);
}

/// The empty list, rendered on its own (the widget alone, on a dock-sized
/// surface). The same state in context is `92-open-project-empty-open`.
#[test]
fn empty_state_proof() {
    use op_editor_ui::widgets::projects_panel_layout::ProjectsPanelLayout;
    use op_editor_ui::widgets::{PaintCx, ProjectsPanel, ProjectsPanelText};
    for mode in BOTH {
        let theme = theme_of(mode);
        let text: ProjectsPanelText<'static> = op_editor_ui::widgets::projects_panel_flow::text(
            &op_editor_core::EditorState::default(),
        );
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
                std::path::Path::new(&dir).join(format!("90-dock-list-empty-{suffix}.png")),
                bytes.as_bytes(),
            )
            .unwrap();
        }
    }
}

/// The whole workspace in one frame: the Projects dock at the right edge,
/// the Design panel beside it, and the open files as tabs in the top bar,
/// for the same three projects.
#[test]
fn projects_and_file_tabs_together_proof() {
    use op_editor_core::DocumentTabView;
    let tab = |n: u64, label: &str, project: &str, status, dirty, active| DocumentTabView {
        id: DocumentId(n),
        label: label.to_string(),
        project: project.to_string(),
        dirty,
        status,
        active,
    };
    use DocumentTabStatus::*;
    for mode in BOTH {
        let mut host = seeded(mode, mixed_projects(), Some("card"));
        host.editor_state_mut().editor_ui.document_tabs = vec![
            tab(1, "checkout.op", "storefront", Running, false, true),
            tab(2, "cart.op", "storefront", NeedsReview, true, false),
            tab(3, "thank-you-page.op", "storefront", Error, false, false),
            tab(4, "onboarding.op", "mobile-app", Done, false, false),
        ];
        let centre = canvas_centre(&host);
        let px = render_probes(&mut host, "91-workspace-projects-and-tabs", &[centre]);
        assert_eq!(px[0], [255, 255, 255], "{mode:?}: artboard is centred");
    }
}
