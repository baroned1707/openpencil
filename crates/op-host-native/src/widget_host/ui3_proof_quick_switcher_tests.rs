//! Proof scenes for the Cmd+K quick switcher: `98-switcher-open`,
//! `98-switcher-query` and `98-switcher-no-match` in both themes
//! (`OP_PROOF_DIR`).

use super::super::ui3_proof::{editor_host, render_probes, PROOF_H, PROOF_W};
use crate::widget_host::WidgetHostNative;
use op_editor_core::{
    DocumentId, DocumentTabStatus, DocumentTabView, ProjectFileKind, ProjectFileView, ProjectView,
    ThemeMode,
};

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

/// The switcher over the editor: four open documents (digits, one unsaved),
/// then closed project files, with the project muted.
fn switcher_host(mode: ThemeMode, query: &str) -> WidgetHostNative {
    let mut host = editor_host(mode, None);
    {
        let ui = &mut host.editor_state_mut().editor_ui;
        ui.document_tabs_supported = true;
        let names = [
            "screen-checkout.op",
            "screen-cart.op",
            "home-hero.op",
            "Untitled",
        ];
        ui.document_tabs = names
            .iter()
            .enumerate()
            .map(|(n, name)| DocumentTabView {
                project: if *name == "Untitled" {
                    String::new()
                } else {
                    "shop".to_string()
                },
                ..tab(n as u64 + 1, name, n == 1, n == 0)
            })
            .collect();
        ui.projects_panel.projects = vec![
            ProjectView {
                root: "/Users/me/Work/shop".to_string(),
                name: "shop".to_string(),
                files: vec![
                    file("screen-checkout.op", Some(1), false, true),
                    file("screen-cart.op", Some(2), true, false),
                    file("home-hero.op", Some(3), false, false),
                    file("screen-pricing.op", None, false, false),
                    file("archive.op", None, false, false),
                ],
                ..ProjectView::default()
            },
            ProjectView {
                root: "/Users/me/Work/blog".to_string(),
                name: "blog".to_string(),
                files: vec![
                    file("home-hero.op", None, false, false),
                    file("screen-archive.op", None, false, false),
                ],
                ..ProjectView::default()
            },
        ];
        ui.projects_panel.switcher.open_fresh(0);
        ui.projects_panel.switcher.input.set_text(query);
    }
    host.mark_dirty();
    host.zoom_to_fit(PROOF_W, PROOF_H);
    host
}

#[test]
fn switcher_empty_query_proof() {
    for mode in BOTH {
        let mut host = switcher_host(mode, "");
        render_probes(&mut host, "98-switcher-open", &[(0, 0)]);
    }
}

#[test]
fn switcher_with_query_and_results_proof() {
    for mode in BOTH {
        let mut host = switcher_host(mode, "scr");
        host.editor_state_mut()
            .editor_ui
            .projects_panel
            .switcher
            .selected = 1;
        render_probes(&mut host, "98-switcher-query", &[(0, 0)]);
    }
}

#[test]
fn switcher_no_match_proof() {
    for mode in BOTH {
        let mut host = switcher_host(mode, "zzz");
        render_probes(&mut host, "98-switcher-no-match", &[(0, 0)]);
    }
}
