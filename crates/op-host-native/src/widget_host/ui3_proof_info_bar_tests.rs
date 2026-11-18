//! Proof scenes for the TopBar info bar that replaced the tab strip:
//! `97-info-bar-*` in both themes (`OP_PROOF_DIR`). Each scene is a band of the
//! real editor frame (the bar plus what hangs under it), not a mock.
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use super::super::ui3_proof::{editor_host, render_probes_at, rgb8};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, ProjectFileKind, ProjectFileView, ProjectView,
    ThemeMode, TopBarButton,
};
use op_editor_ui::widgets::{TopBar, TOP_BAR_HEIGHT};
use op_editor_ui::{Point2D, Rect, Theme};

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];
/// The band that is rendered: the bar and the page menu hanging under it.
const BAND_H: f32 = 300.0;

fn tab(
    id: u64,
    label: &str,
    dirty: bool,
    status: DocumentTabStatus,
    active: bool,
) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: "storefront".to_string(),
        dirty,
        status,
        active,
    }
}

/// What a scene says about the active file.
struct Scene {
    project: &'static str,
    folder: &'static str,
    file: &'static str,
    branch: Option<&'static str>,
    dirty: bool,
    pages: usize,
    other_unsaved: usize,
    running_in: Option<&'static str>,
}

impl Scene {
    fn wide() -> Self {
        Self {
            project: "storefront",
            folder: "landing/v2",
            file: "hero-section.op",
            branch: Some("main"),
            dirty: false,
            pages: 5,
            other_unsaved: 0,
            running_in: None,
        }
    }
}

fn host_for(mode: ThemeMode, width: f32, scene: &Scene) -> WidgetHostNative {
    let mut host = editor_host(mode, None);
    host.last_viewport_w = width;
    host.last_viewport_h = BAND_H;
    while host.editor_state().page_count() < scene.pages {
        host.editor_state_mut().add_page().expect("page added");
    }
    host.editor_state_mut().ui.active_page_index = 1.min(scene.pages - 1);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.file_name_display = Some(scene.file.to_string());
    ui.document_dirty = scene.dirty;
    ui.git_panel.branch = scene.branch.map(str::to_string);
    let mut tabs = vec![tab(
        1,
        scene.file,
        scene.dirty,
        DocumentTabStatus::Idle,
        true,
    )];
    for n in 0..scene.other_unsaved {
        tabs.push(tab(
            10 + n as u64,
            &format!("screen-{n}.op"),
            true,
            DocumentTabStatus::Idle,
            false,
        ));
    }
    if let Some(name) = scene.running_in {
        tabs.push(tab(99, name, false, DocumentTabStatus::Running, false));
    }
    ui.document_tabs = tabs;
    ui.projects_panel.projects = vec![ProjectView {
        root: "/Users/me/Work/storefront".to_string(),
        name: scene.project.to_string(),
        files: vec![ProjectFileView {
            path: format!("/Users/me/Work/storefront/{}/{}", scene.folder, scene.file),
            label: scene.file.to_string(),
            open_id: Some(DocumentId(1)),
            dirty: scene.dirty,
            status: DocumentTabStatus::Idle,
            active: true,
            folder: scene.folder.to_string(),
            kind: ProjectFileKind::Design,
            missing: false,
            removable: false,
        }],
        ..ProjectView::default()
    }];
    host.mark_dirty();
    host
}

fn theme_of(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::Light => Theme::light(),
    }
}

fn button_centre(host: &WidgetHostNative, width: f32, button: TopBarButton) -> Option<(i32, i32)> {
    let bar = TopBar::for_editor_state(host.editor_state());
    let rect = Rect::xywh(0.0, 0.0, width, TOP_BAR_HEIGHT);
    let at = host.topbar_button_rect(&bar, rect, button)?;
    let centre = Point2D::new(at.origin.x + at.size.x / 2.0, at.origin.y + at.size.y / 2.0);
    Some((centre.x as i32, centre.y as i32))
}

fn render(host: &mut WidgetHostNative, name: &str, width: f32) {
    render_probes_at(host, name, &[(0, 0)], width, BAND_H);
}

#[test]
fn wide_bar_with_project_folder_file_and_saved_proof() {
    for mode in BOTH {
        let mut host = host_for(mode, 1440.0, &Scene::wide());
        render(&mut host, "97-info-bar-wide", 1440.0);
    }
}

#[test]
fn dirty_file_with_two_other_unsaved_proof() {
    for mode in BOTH {
        let scene = Scene {
            dirty: true,
            other_unsaved: 2,
            ..Scene::wide()
        };
        let mut host = host_for(mode, 1440.0, &scene);
        render(&mut host, "97-info-bar-dirty", 1440.0);
    }
}

#[test]
fn agent_running_in_another_tab_proof() {
    for mode in BOTH {
        let scene = Scene {
            dirty: true,
            other_unsaved: 1,
            running_in: Some("checkout-flow.op"),
            ..Scene::wide()
        };
        let mut host = host_for(mode, 1440.0, &scene);
        // The dot of the agent item is painted in the running colour.
        let dot = button_centre(&host, 1440.0, TopBarButton::AgentActivity).expect("agent item");
        let bar = TopBar::for_editor_state(host.editor_state());
        let rect = Rect::xywh(0.0, 0.0, 1440.0, TOP_BAR_HEIGHT);
        let cell = host
            .topbar_button_rect(&bar, rect, TopBarButton::AgentActivity)
            .unwrap();
        let dot_probe = (cell.origin.x as i32 + 6 + 3, dot.1);
        let px = render_probes_at(&mut host, "97-info-bar-agent", &[dot_probe], 1440.0, BAND_H)[0];
        assert_eq!(
            px,
            rgb8(theme_of(mode).primary),
            "{mode:?}: the running dot"
        );
    }
}

#[test]
fn long_names_are_elided_proof() {
    for mode in BOTH {
        let scene = Scene {
            project: "monorepo-storefront-checkout-redesign",
            folder: "marketing/campaigns/spring-sale-2027",
            file: "hero-section-with-a-very-long-descriptive-name-v12-final.op",
            branch: Some("feature/a-long-branch-name"),
            dirty: true,
            other_unsaved: 3,
            running_in: Some("another-extremely-long-file-name-in-a-different-tab.op"),
            ..Scene::wide()
        };
        let mut host = host_for(mode, 1440.0, &scene);
        render(&mut host, "97-info-bar-long", 1440.0);
    }
}

#[test]
fn narrow_windows_drop_items_in_order_proof() {
    for mode in BOTH {
        let scene = Scene {
            dirty: true,
            other_unsaved: 3,
            running_in: Some("checkout-flow.op"),
            ..Scene::wide()
        };
        for width in [960.0, 640.0] {
            let mut host = host_for(mode, width, &scene);
            render(&mut host, &format!("97-info-bar-narrow-{width:.0}"), width);
        }
    }
}

#[test]
fn page_menu_open_under_the_chip_proof() {
    for mode in BOTH {
        let scene = Scene {
            dirty: true,
            pages: 7,
            ..Scene::wide()
        };
        let mut host = host_for(mode, 1440.0, &scene);
        let chip = button_centre(&host, 1440.0, TopBarButton::PageChip).expect("page chip");
        assert!(host.apply_press(chip.0 as f32, chip.1 as f32, 1440.0, BAND_H));
        assert!(host.page_menu_open());
        // Hover the third row so the wash shows.
        let popup = host.page_menu_popup_rect().expect("menu is open");
        host.apply_cursor_move(
            popup.origin.x + 30.0,
            popup.origin.y + popup.size.y / 7.0 * 2.5,
        );
        render(&mut host, "97-info-bar-page-menu", 1440.0);
    }
}

#[test]
fn breadcrumb_hover_wash_proof() {
    for mode in BOTH {
        let mut host = host_for(mode, 1440.0, &Scene::wide());
        host.editor_state_mut().editor_ui.topbar_button_hover = Some(TopBarButton::Breadcrumb);
        render(&mut host, "97-info-bar-hover", 1440.0);
    }
}
