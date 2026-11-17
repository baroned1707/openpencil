//! Proof scenes for the Projects dock's file actions: the hover pencil + trash,
//! the inline "Move to Trash?" question, "New design here" on a folder and on
//! the project header, a freshly created row in rename mode, an open tab whose
//! file is not on disk, and the Settings switch for autosave.
//!
//! Writes `.proof-of-work/95-<scene>-<theme>.png` when `OP_PROOF_DIR` is set.

use super::super::ui3_proof::{editor_host, render_probes, render_scene, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::agent_settings::AgentSettingsTab;
use op_editor_core::{
    DocumentId, DocumentTabStatus, ProjectFileKind, ProjectFileView, ProjectRowTarget, ProjectView,
    ThemeMode,
};
use op_editor_ui::widgets::projects_dock_flow as dock_flow;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];
const ROOT: &str = "/Users/me/Work/storefront";

fn file(folder: &str, label: &str, open: Option<u64>) -> ProjectFileView {
    ProjectFileView {
        path: format!("{ROOT}/{folder}/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: open == Some(1),
        folder: folder.to_string(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: true,
    }
}

/// An opened project: a root file and two folders.
fn storefront(files: Vec<ProjectFileView>) -> ProjectView {
    ProjectView {
        root: ROOT.to_string(),
        name: "storefront".to_string(),
        files,
        closable: true,
        ..ProjectView::default()
    }
}

fn default_files() -> Vec<ProjectFileView> {
    vec![
        file("", "brand.op", None),
        file("designs", "checkout.op", Some(1)),
        file("designs", "cart.op", None),
        file("designs", "thank-you.op", None),
        file("screens", "home.op", None),
        file("screens", "search.op", None),
    ]
}

/// Render the dock with `project` in both themes; `prepare` sets the panel state.
fn scene(name: &str, project: ProjectView, prepare: impl Fn(&mut WidgetHostNative)) {
    for mode in BOTH {
        let mut host = editor_host(mode, None);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.document_tabs_supported = true;
        ui.projects_panel.projects = vec![project.clone()];
        prepare(&mut host);
        host.mark_dirty();
        host.zoom_to_fit(PROOF_W, PROOF_H);
        assert!(
            dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H).is_some(),
            "{mode:?}: the dock is shown"
        );
        render_probes(&mut host, name, &[(0, 0)]);
    }
}

fn hover(host: &mut WidgetHostNative, target: ProjectRowTarget) {
    host.editor_state_mut().editor_ui.projects_panel.hover = Some(target);
}

#[test]
fn row_hover_shows_the_pencil_and_the_trash_proof() {
    scene(
        "95-row-hover-actions",
        storefront(default_files()),
        |host| {
            hover(host, ProjectRowTarget::File(0, 2));
        },
    );
    scene("95-row-hover-trash", storefront(default_files()), |host| {
        hover(host, ProjectRowTarget::TrashFile(0, 2));
    });
}

#[test]
fn a_row_asking_move_to_trash_proof() {
    scene("95-trash-confirm", storefront(default_files()), |host| {
        let panel = &mut host.editor_state_mut().editor_ui.projects_panel;
        assert!(panel.begin_trash_confirm(&format!("{ROOT}/designs/cart.op")));
    });
    scene(
        "95-trash-confirm-hover",
        storefront(default_files()),
        |host| {
            let panel = &mut host.editor_state_mut().editor_ui.projects_panel;
            assert!(panel.begin_trash_confirm(&format!("{ROOT}/designs/cart.op")));
            hover(host, ProjectRowTarget::TrashFile(0, 2));
        },
    );
}

#[test]
fn new_design_here_on_a_folder_and_on_the_header_proof() {
    scene("95-folder-new-here", storefront(default_files()), |host| {
        hover(host, ProjectRowTarget::NewHere(0, Some(1)));
    });
    scene("95-header-new-here", storefront(default_files()), |host| {
        hover(host, ProjectRowTarget::NewHere(0, None));
    });
}

#[test]
fn a_freshly_created_design_is_in_rename_mode_proof() {
    let mut files = default_files();
    files.insert(1, file("designs", "Untitled.op", None));
    // `designs` rows: Untitled.op, checkout.op, ... sorted as the host would.
    files.sort_by(|a, b| a.folder.cmp(&b.folder));
    scene("95-created-rename", storefront(files), |host| {
        let panel = &mut host.editor_state_mut().editor_ui.projects_panel;
        assert!(panel.begin_rename(&format!("{ROOT}/designs/Untitled.op"), 0));
    });
}

#[test]
fn an_open_tab_whose_file_left_the_disk_is_marked_proof() {
    let mut files = default_files();
    files[1].missing = true;
    files[1].dirty = true;
    scene("95-not-on-disk", storefront(files), |host| {
        hover(host, ProjectRowTarget::File(0, 2));
    });
}

#[test]
fn settings_system_tab_with_the_autosave_switch_proof() {
    for mode in BOTH {
        let mut host = editor_host(mode, None);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.agent_settings_open = true;
        ui.agent_settings.tab = AgentSettingsTab::System;
        ui.agent_settings.autosave_on_agent_run_end = true;
        let pixel = render_scene(&mut host, "95-settings-autosave-on", (720, 450));
        assert_ne!(pixel, [0, 0, 0], "{mode:?} painted nothing");
        host.editor_state_mut()
            .editor_ui
            .agent_settings
            .autosave_on_agent_run_end = false;
        render_scene(&mut host, "95-settings-autosave-off", (720, 450));
    }
}
