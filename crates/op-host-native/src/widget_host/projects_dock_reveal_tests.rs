//! Scrolling one file's dock row into view (the tail of the breadcrumb's
//! "show in the dock"). The unfolding is the desktop host's job; here the rows
//! are already open.
//!
//! Windows-gated like the other tests that solve layout.

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::WidgetHostNative;
use op_editor_core::{EditorState, ProjectFileView, ProjectView};
use op_editor_ui::widgets::projects_panel_layout::RowKind;

const VW: f32 = 1_400.0;
const VH: f32 = 600.0;

fn file(n: usize) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/designs/screen-{n:02}.op"),
        label: format!("screen-{n:02}.op"),
        open_id: None,
        dirty: false,
        status: op_editor_core::DocumentTabStatus::Idle,
        active: false,
        folder: "designs".to_string(),
        kind: op_editor_core::ProjectFileKind::Design,
        missing: false,
        removable: false,
    }
}

/// A host whose dock lists `count` files, more than fit on screen.
fn host(count: usize) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.install_imported_state(EditorState::starter());
    let ui = &mut host.editor_state.editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: (0..count).map(file).collect(),
        ..ProjectView::default()
    }];
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    host.mark_dirty();
    host
}

fn path_of(n: usize) -> String {
    file(n).path
}

fn row_is_visible(host: &WidgetHostNative, n: usize) -> bool {
    let layout = host.projects_dock_layout(VW, VH).expect("the dock shows");
    let list = layout.list.expect("the list shows");
    let row = list.row(RowKind::File(0, n)).expect("row is laid out").rect;
    row.origin.y >= list.panel.origin.y
        && row.origin.y + row.size.y <= list.panel.origin.y + list.panel.size.y + 0.5
}

#[test]
fn a_row_below_the_fold_is_scrolled_into_view() {
    let mut host = host(60);
    assert!(
        !row_is_visible(&host, 55),
        "fixture: the row starts off screen"
    );
    assert!(host.reveal_project_file(&path_of(55)));
    assert!(row_is_visible(&host, 55));
    assert!(host.editor_state.editor_ui.projects_panel.scroll.offset > 0.0);
}

#[test]
fn a_row_already_in_view_does_not_move_the_list() {
    let mut host = host(60);
    let before = host.editor_state.editor_ui.projects_panel.scroll.offset;
    assert!(!host.reveal_project_file(&path_of(1)));
    assert_eq!(
        host.editor_state.editor_ui.projects_panel.scroll.offset,
        before
    );
}

#[test]
fn a_file_without_a_row_reveals_nothing() {
    let mut host = host(60);
    assert!(!host.reveal_project_file("/work/shop/designs/deleted.op"));
}

#[test]
fn a_hidden_row_cannot_be_scrolled_to() {
    // Folded project: the row is not laid out, so the host must unfold first.
    let mut host = host(60);
    host.editor_state.editor_ui.projects_panel.projects[0].expanded = false;
    assert!(!host.reveal_project_file(&path_of(55)));
}
