//! Proof scenes for "Open project" — the Projects dock with a folder the user
//! opened: the empty dock (slim strip and unfolded), folders grouped under
//! sub-headers (some folded), the scanning / slow / truncated status lines, the
//! header's refresh and close buttons, and long folder and file names.
//!
//! The `93-fig-*` scenes show the same dock with Figma `.fig` files listed among
//! the `.op` files: every conversion status, the "Convert all" rows and a project
//! that holds nothing but `.fig` files.
//!
//! Writes `.proof-of-work/92-open-project-<scene>-<theme>.png` (and
//! `93-fig-<scene>-<theme>.png`) when `OP_PROOF_DIR` is set.

use super::super::ui3_proof::{editor_host, render_probes, rgb8, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, FigStatus, ProjectFileKind, ProjectFileView, ProjectRowTarget,
    ProjectScanState, ProjectView, ThemeMode,
};
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::projects_dock_flow as dock_flow;
use op_editor_ui::Theme;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn theme_of(mode: ThemeMode) -> Theme {
    match mode {
        ThemeMode::Dark => Theme::dark(),
        ThemeMode::Light => Theme::light(),
    }
}

fn file(
    folder: &str,
    label: &str,
    open: Option<u64>,
    status: DocumentTabStatus,
) -> ProjectFileView {
    ProjectFileView {
        path: format!("/Users/me/Work/storefront/{folder}/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status,
        active: open == Some(1),
        folder: folder.to_string(),
        kind: op_editor_core::ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

/// A project the user opened: files in the root and in three nested folders.
fn storefront() -> ProjectView {
    use DocumentTabStatus::*;
    ProjectView {
        root: "/Users/me/Work/storefront".to_string(),
        name: "storefront".to_string(),
        files: vec![
            file("", "brand.op", None, Idle),
            file("designs", "checkout.op", Some(1), Running),
            file("designs", "cart.op", Some(2), NeedsReview),
            file("designs", "thank-you.op", None, Idle),
            file("screens", "home.op", None, Idle),
            file("screens", "search.op", None, Idle),
            file("screens/auth", "login.op", Some(3), Idle),
            file("screens/auth", "signup.op", None, Idle),
            file("screens/auth", "reset-password.op", None, Idle),
            file("screens/auth/legacy", "old-login.op", None, Idle),
        ],
        closable: true,
        collapsed_folders: vec!["screens".to_string(), "screens/auth/legacy".to_string()],
        ..ProjectView::default()
    }
}

/// An inferred project beside it, as the dock lists those.
fn inferred() -> ProjectView {
    ProjectView {
        root: "/Users/me/Work/blog".to_string(),
        name: "blog".to_string(),
        files: vec![file("designs", "post.op", None, DocumentTabStatus::Idle)],
        ..ProjectView::default()
    }
}

fn seeded(mode: ThemeMode, projects: Vec<ProjectView>) -> WidgetHostNative {
    let mut host = editor_host(mode, None);
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = projects;
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

fn each_theme(
    name: &str,
    projects: Vec<ProjectView>,
    prepare: impl Fn(&mut WidgetHostNative),
) -> Vec<(ThemeMode, WidgetHostNative)> {
    BOTH.iter()
        .map(|&mode| {
            let mut host = seeded(mode, projects.clone());
            prepare(&mut host);
            host.mark_dirty();
            host.zoom_to_fit(PROOF_W, PROOF_H);
            let dock = dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H)
                .expect("the dock is shown")
                .rect;
            // One pixel inside the dock's bare lower surface: its own tone.
            let probe = ((dock.origin.x + 3.0) as i32, (PROOF_H - 6.0) as i32);
            let pixel = render_probes(&mut host, name, &[probe])[0];
            assert_eq!(
                pixel,
                rgb8(theme_of(mode).muted),
                "{mode:?}: the dock's tone"
            );
            (mode, host)
        })
        .collect()
}

#[test]
fn empty_dock_collapsed_strip_with_the_folder_plus_icon_proof() {
    for (mode, host) in each_theme("92-open-project-empty-strip", Vec::new(), |_| {}) {
        let dock = dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H).unwrap();
        assert!(!dock.open, "{mode:?}: an empty dock is a strip");
        assert_eq!(dock.rect.size.x, 28.0);
        assert!(
            dock.open_project.is_some(),
            "{mode:?}: the strip offers Open project"
        );
        let canvas = canvas_geometry::canvas_rect(host.editor_state(), PROOF_W, PROOF_H);
        assert_eq!(canvas.origin.x + canvas.size.x, PROOF_W - 28.0);
    }
}

#[test]
fn empty_dock_unfolded_with_the_open_project_button_proof() {
    for (mode, host) in each_theme("92-open-project-empty-open", Vec::new(), |host| {
        host.editor_state_mut().editor_ui.projects_panel.empty_open = true;
    }) {
        let dock = dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H).unwrap();
        assert!(dock.open, "{mode:?}");
        let button = dock.list.unwrap().empty_button.expect("the big button");
        assert!(button.size.x > 100.0);
    }
}

#[test]
fn opened_project_with_nested_folders_some_folded_proof() {
    each_theme(
        "92-open-project-tree",
        vec![storefront(), inferred()],
        |_| {},
    );
}

#[test]
fn a_scanning_project_proof() {
    let mut scanning = storefront();
    scanning.files.clear();
    scanning.scan = ProjectScanState::Scanning;
    each_theme("92-open-project-scanning", vec![scanning], |_| {});
    let mut slow = storefront();
    slow.scan = ProjectScanState::Slow;
    each_theme("92-open-project-slow", vec![slow], |_| {});
}

#[test]
fn a_truncated_project_says_how_many_are_not_shown_proof() {
    let mut cut = storefront();
    cut.scan = ProjectScanState::Truncated(1240);
    cut.collapsed_folders.clear();
    each_theme("92-open-project-truncated", vec![cut], |_| {});
}

#[test]
fn an_empty_opened_folder_offers_new_design_proof() {
    let mut empty = storefront();
    empty.files.clear();
    each_theme("92-open-project-no-designs", vec![empty], |_| {});
}

#[test]
fn hovering_the_project_header_shows_refresh_and_close_with_a_tooltip_proof() {
    each_theme(
        "92-open-project-hover-header",
        vec![storefront(), inferred()],
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::Header(0));
        },
    );
    each_theme(
        "92-open-project-hover-close",
        vec![storefront(), inferred()],
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::Close(0));
        },
    );
    each_theme(
        "92-open-project-hover-folder",
        vec![storefront(), inferred()],
        |host| {
            host.editor_state_mut().editor_ui.projects_panel.hover =
                Some(ProjectRowTarget::Folder(0, 1));
        },
    );
}

#[test]
fn long_folder_and_file_names_are_ellipsized_proof() {
    use DocumentTabStatus::*;
    let long = ProjectView {
        root: "/Users/me/Work/a-repository-with-a-remarkably-long-name".to_string(),
        name: "a-repository-with-a-remarkably-long-name".to_string(),
        files: vec![
            file(
                "",
                "an-extremely-long-design-file-name-for-the-checkout-flow.op",
                Some(1),
                NeedsReview,
            ),
            file(
                "packages/storefront/apps/web/src/screens/checkout/payment-methods",
                "credit-card-form-with-a-very-long-name.op",
                None,
                Idle,
            ),
            file("设计稿/首页/最终版", "首页-设计稿-请勿删除.op", None, Idle),
        ],
        closable: true,
        ..ProjectView::default()
    };
    each_theme("92-open-project-long-names", vec![long], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover = Some(ProjectRowTarget::Header(0));
    });
}

/// A fresh launch shows Home with no project. The always-present strip must not
/// paint over Home's surface or take a click meant for it.
#[test]
fn home_with_an_empty_dock_keeps_its_clicks_proof() {
    for mode in BOTH {
        let mut host = WidgetHostNative::new();
        host.editor_state_mut().editor_ui.theme_mode = mode;
        host.editor_state_mut().editor_ui.document_tabs_supported = true;
        host.editor_state_mut().editor_ui.home.visible = true;
        assert!(host.home_visible(), "{mode:?}: Home shows");
        host.mark_dirty();
        let dock = dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H)
            .expect("the empty dock is laid out under Home");
        let button = dock.open_project.expect("the strip's folder-plus");
        let at = (button.origin.x + 4.0, button.origin.y + 4.0);
        // Home's own surface at the strip's x: what paints there must be Home's.
        render_probes(&mut host, "92-open-project-home-empty-dock", &[(0, 0)]);
        host.apply_press(at.0, at.1, PROOF_W, PROOF_H);
        host.apply_release_with_viewport(PROOF_W, PROOF_H);
        assert_eq!(
            host.editor_state().editor_ui.projects_panel.pending_request,
            None,
            "{mode:?}: Home owns the pointer, the strip behind it asks for nothing"
        );
    }
}

fn fig(folder: &str, label: &str, status: FigStatus) -> ProjectFileView {
    ProjectFileView {
        kind: ProjectFileKind::Figma(status),
        ..file(folder, label, None, DocumentTabStatus::Idle)
    }
}

/// A project mixing `.op` designs with `.fig` exports in every status.
fn with_figs() -> ProjectView {
    use DocumentTabStatus::Idle;
    ProjectView {
        root: "/Users/me/Work/storefront".to_string(),
        name: "storefront".to_string(),
        files: vec![
            fig("", "brand-kit.fig", FigStatus::NotConverted),
            file("", "brand.op", None, Idle),
            fig("designs", "checkout.fig", FigStatus::Converted),
            file(
                "designs",
                "checkout.op",
                Some(1),
                DocumentTabStatus::Running,
            ),
            fig("designs", "cart.fig", FigStatus::Converting),
            fig(
                "designs",
                "wishlist.fig",
                FigStatus::Failed("timed out".to_string()),
            ),
            fig("imports", "marketing-site.fig", FigStatus::NotConverted),
            fig("imports", "icons.fig", FigStatus::NotConverted),
            fig("imports", "illustrations.fig", FigStatus::NotConverted),
            file("imports", "notes.op", None, Idle),
        ],
        closable: true,
        ..ProjectView::default()
    }
}

#[test]
fn fig_files_listed_among_op_files_in_every_status_proof() {
    each_theme("93-fig-tree", vec![with_figs(), inferred()], |_| {});
}

#[test]
fn a_folder_header_offers_convert_all_with_its_count_proof() {
    let mut project = with_figs();
    // The root's own fig is done, so only the `imports` folder has a row.
    project.files[0].kind = ProjectFileKind::Figma(FigStatus::Converted);
    each_theme("93-fig-folder-convert-all", vec![project.clone()], |_| {});
    let rows = dock_flow::layout(
        seeded(ThemeMode::Dark, vec![project]).editor_state(),
        PROOF_W,
        PROOF_H,
    )
    .unwrap()
    .list
    .unwrap();
    assert!(
        rows.rows.iter().any(|row| matches!(
            row.kind,
            op_editor_ui::widgets::projects_panel_layout::RowKind::ConvertAll(0, Some(_))
        )),
        "the folder carries its own Convert all row"
    );
}

#[test]
fn a_project_of_only_fig_files_proof() {
    let only = ProjectView {
        root: "/Users/me/Work/exports".to_string(),
        name: "exports".to_string(),
        files: vec![
            fig("", "landing-v1.fig", FigStatus::NotConverted),
            fig("", "landing-v2.fig", FigStatus::NotConverted),
            fig("", "app-screens.fig", FigStatus::NotConverted),
            fig(
                "archive",
                "old-brand.fig",
                FigStatus::Failed("not a regular file".to_string()),
            ),
        ],
        closable: true,
        ..ProjectView::default()
    };
    each_theme("93-fig-only-figs", vec![only], |_| {});
}

#[test]
fn hovering_fig_rows_and_convert_all_shows_the_wash_and_tooltip_proof() {
    // Rows: header, project Convert all, brand-kit.fig, ...
    each_theme("93-fig-hover-row", vec![with_figs()], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover = Some(ProjectRowTarget::File(0, 0));
    });
    each_theme("93-fig-hover-convert-all", vec![with_figs()], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover =
            Some(ProjectRowTarget::ConvertAll(0, None));
    });
    each_theme("93-fig-hover-failed", vec![with_figs()], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover = Some(ProjectRowTarget::File(0, 5));
    });
}

// ---- 94-rename: renaming a design from the dock -----------------------------

const CART: &str = "/Users/me/Work/storefront/designs/cart.op";

/// Open the inline rename on `path` with `draft` typed in.
fn renaming(host: &mut WidgetHostNative, path: &str, draft: Option<&str>) {
    let panel = &mut host.editor_state_mut().editor_ui.projects_panel;
    assert!(panel.begin_rename(path, 0), "{path} is a renamable row");
    if let Some(draft) = draft {
        panel.rename.as_mut().unwrap().input.set_text(draft);
        panel.refresh_rename_error();
    }
}

#[test]
fn rename_edit_mode_with_the_stem_selected_proof() {
    for (mode, host) in each_theme("94-rename-edit", vec![storefront(), inferred()], |host| {
        renaming(host, CART, None);
    }) {
        let edit = host
            .editor_state()
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap();
        assert_eq!(
            edit.input.text(),
            "cart",
            "{mode:?}: no extension in the field"
        );
        assert_eq!(
            edit.input.highlight_range(),
            Some((0, 4)),
            "{mode:?}: stem selected"
        );
    }
}

#[test]
fn rename_edit_mode_with_a_collision_message_proof() {
    for (mode, host) in each_theme(
        "94-rename-collision",
        vec![storefront(), inferred()],
        |host| renaming(host, CART, Some("checkout")),
    ) {
        let edit = host
            .editor_state()
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap();
        assert_eq!(
            edit.error,
            Some(op_editor_core::RenameNameError::Taken),
            "{mode:?}: a sibling is called checkout.op"
        );
    }
}

#[test]
fn rename_edit_mode_with_an_invalid_name_proof() {
    each_theme("94-rename-invalid", vec![storefront()], |host| {
        renaming(host, CART, Some("cart/v2"));
    });
}

#[test]
fn rename_edit_mode_with_a_long_name_clips_inside_the_field_proof() {
    let long = "an-extremely-long-design-file-name-for-the-checkout-flow-final-v12";
    for (mode, host) in each_theme("94-rename-long", vec![storefront()], |host| {
        renaming(host, CART, Some(long));
    }) {
        let edit = host
            .editor_state()
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap();
        assert_eq!(
            edit.error, None,
            "{mode:?}: a long name is still a valid name"
        );
    }
}

#[test]
fn rename_hover_shows_the_pencil_with_its_tooltip_proof() {
    each_theme("94-rename-hover-pencil", vec![storefront()], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover =
            Some(ProjectRowTarget::RenameFile(0, 2));
    });
    each_theme("94-rename-hover-row", vec![storefront()], |host| {
        host.editor_state_mut().editor_ui.projects_panel.hover = Some(ProjectRowTarget::File(0, 3));
    });
}

#[test]
fn rename_result_row_shows_the_new_name_proof() {
    let mut renamed = storefront();
    // What the host publishes after the rename: the label and path changed,
    // the tab (cart.op was open and is still open) follows.
    renamed.files[2].label = "cart-v2.op".to_string();
    renamed.files[2].path = "/Users/me/Work/storefront/designs/cart-v2.op".to_string();
    each_theme("94-rename-result", vec![renamed], |_| {});
}
