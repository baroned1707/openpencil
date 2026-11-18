//! Proof scenes for the dock's open-document marks: `98-dock-open-marks`,
//! `98-dock-hover-close`, `98-dock-no-project*` in both themes (`OP_PROOF_DIR`).

use super::super::ui3_proof::{editor_host, render_probes, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, ProjectFileKind, ProjectFileView,
    ProjectRowTarget, ProjectView, ThemeMode,
};
use op_editor_ui::widgets::projects_dock_flow as dock_flow;
use op_editor_ui::widgets::projects_panel_layout::RowKind;

const BOTH: [ThemeMode; 2] = [ThemeMode::Dark, ThemeMode::Light];

fn file(label: &str, open: Option<u64>, dirty: bool, active: bool) -> ProjectFileView {
    ProjectFileView {
        path: format!("/Users/me/Work/shop/designs/{label}"),
        label: label.to_string(),
        open_id: open.map(DocumentId),
        dirty,
        status: DocumentTabStatus::Idle,
        active,
        folder: String::new(),
        kind: ProjectFileKind::Design,
        missing: false,
        removable: true,
    }
}

fn tab(id: u64, label: &str, dirty: bool, active: bool) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(id),
        label: label.to_string(),
        project: String::new(),
        dirty,
        status: DocumentTabStatus::Idle,
        active,
    }
}

/// Eleven open files in one project (digits 1..8, 9 on the last, a dot on the
/// two between), one closed file, and one unsaved.
fn open_marks_host(mode: ThemeMode) -> WidgetHostNative {
    let mut host = editor_host(mode, None);
    let mut files: Vec<ProjectFileView> = (1..=11_u64)
        .map(|n| file(&format!("screen-{n:02}.op"), Some(n), n == 3, n == 2))
        .collect();
    files.push(file("archive.op", None, false, false));
    let ui = &mut host.editor_state_mut().editor_ui;
    ui.document_tabs_supported = true;
    ui.document_tabs = (1..=11_u64)
        .map(|n| tab(n, &format!("screen-{n:02}.op"), n == 3, n == 2))
        .collect();
    ui.projects_panel.projects = vec![ProjectView {
        root: "/Users/me/Work/shop".to_string(),
        name: "shop".to_string(),
        files,
        ..ProjectView::default()
    }];
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

#[test]
fn dock_open_marks_proof() {
    for mode in BOTH {
        let mut host = open_marks_host(mode);
        render_probes(&mut host, "98-dock-open-marks", &[(0, 0)]);
    }
}

#[test]
fn dock_hover_close_proof() {
    for mode in BOTH {
        let mut host = open_marks_host(mode);
        host.editor_state_mut().editor_ui.projects_panel.hover =
            Some(ProjectRowTarget::CloseFile(0, 3));
        render_probes(&mut host, "98-dock-hover-close", &[(0, 0)]);
        // The hovered open row shows the close button where the trash would be.
        let layout =
            dock_flow::layout(host.editor_state(), PROOF_W, PROOF_H).expect("dock is shown");
        let row = layout
            .list
            .as_ref()
            .and_then(|list| list.row(RowKind::File(0, 3)))
            .expect("row");
        assert!(row.close.is_some() && row.trash.is_none());
    }
}

#[test]
fn dock_no_project_section_proof() {
    for mode in BOTH {
        let mut host = editor_host(mode, None);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.document_tabs_supported = true;
        ui.document_tabs = vec![
            tab(1, "screen-01.op", false, false),
            tab(2, "Untitled", true, true),
            tab(3, "scratch-pad.op", false, false),
        ];
        ui.projects_panel.projects = vec![ProjectView {
            root: "/Users/me/Work/shop".to_string(),
            name: "shop".to_string(),
            files: vec![file("screen-01.op", Some(1), false, false)],
            ..ProjectView::default()
        }];
        host.mark_dirty();
        host.zoom_to_fit(PROOF_W, PROOF_H);
        render_probes(&mut host, "98-dock-no-project", &[(0, 0)]);
        // Hovering a row of the section shows its close button.
        host.editor_state_mut().editor_ui.projects_panel.hover =
            Some(ProjectRowTarget::CloseOpenDocument(2));
        render_probes(&mut host, "98-dock-no-project-hover", &[(0, 0)]);
    }
}

#[test]
fn dock_no_project_at_all_proof() {
    for mode in BOTH {
        let mut host = editor_host(mode, None);
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.document_tabs_supported = true;
        ui.projects_panel.empty_open = true;
        ui.document_tabs = vec![tab(1, "Untitled", false, true)];
        host.mark_dirty();
        host.zoom_to_fit(PROOF_W, PROOF_H);
        render_probes(&mut host, "98-dock-no-project-at-all", &[(0, 0)]);
    }
}
