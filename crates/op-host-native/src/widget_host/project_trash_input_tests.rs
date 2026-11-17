//! Native-host routing of the Projects dock's "Move to Trash?" question: the
//! trash button asks, the confirm button hands the path to the host, Escape or a
//! press anywhere else cancels, and nothing is ever removed by a stray key.
//!
//! Windows-gated like the other tests that solve layout (see
//! `project_rename_input_tests.rs`).

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::WidgetHostNative;
use op_editor_core::{EditorState, ProjectFileView, ProjectView};
use op_editor_ui::widgets::projects_panel_layout::RowKind;
use op_editor_ui::Point2D;

const VW: f32 = 1_400.0;
const VH: f32 = 900.0;
const CART: &str = "/work/shop/designs/cart.op";

const BOARD: &str = r##"{ "version": "1.0.0", "children": [
    { "type": "frame", "id": "board", "name": "Board", "x": 0, "y": 0,
      "width": 4000, "height": 500, "fill": [{"type":"solid","color":"#ffffff"}],
      "children": [] } ] }"##;

fn file(label: &str, removable: bool) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/designs/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: op_editor_core::DocumentTabStatus::Idle,
        active: false,
        folder: "designs".to_string(),
        kind: op_editor_core::ProjectFileKind::Design,
        missing: false,
        removable,
    }
}

fn host() -> WidgetHostNative {
    let document = jian_ops_schema::load_str(BOARD).expect("fixture").value;
    let mut host = WidgetHostNative::new();
    host.install_imported_state(EditorState::from_document(document));
    let ui = &mut host.editor_state.editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![file("cart.op", true), file("checkout.op", true)],
        closable: true,
        ..ProjectView::default()
    }];
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    host.editor_state.viewport = op_editor_core::Viewport::IDENTITY;
    host.mark_dirty();
    host
}

fn centre(rect: op_editor_ui::Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

/// The trash button of file row `file` (also the confirm button while it asks).
fn trash_button(host: &WidgetHostNative, file: usize) -> Point2D {
    let layout = host.projects_dock_layout(VW, VH).expect("the dock shows");
    let list = layout.list.expect("the list shows");
    centre(
        list.row(RowKind::File(0, file))
            .and_then(|row| row.trash)
            .expect("trash button"),
    )
}

fn cancel_button(host: &WidgetHostNative, file: usize) -> Point2D {
    let layout = host.projects_dock_layout(VW, VH).expect("the dock shows");
    let list = layout.list.expect("the list shows");
    centre(
        list.row(RowKind::File(0, file))
            .and_then(|row| row.rename)
            .expect("pencil slot"),
    )
}

fn press_at(host: &mut WidgetHostNative, point: Point2D) -> bool {
    host.apply_cursor_move(point.x, point.y);
    let consumed = host.apply_press(point.x, point.y, VW, VH);
    host.apply_release_with_viewport(VW, VH);
    consumed
}

fn panel(host: &WidgetHostNative) -> &op_editor_core::editor_ui_state::ProjectsPanelState {
    &host.editor_state.editor_ui.projects_panel
}

#[test]
fn the_trash_button_asks_and_the_confirm_button_hands_the_path_to_the_host() {
    let mut host = host();
    assert!({
        let at = trash_button(&host, 0);
        press_at(&mut host, at)
    });
    assert_eq!(panel(&host).trash_confirm.as_deref(), Some(CART));
    assert_eq!(panel(&host).trash_commit, None, "asking removes nothing");

    assert!({
        let at = trash_button(&host, 0);
        press_at(&mut host, at)
    });
    assert_eq!(panel(&host).trash_commit.as_deref(), Some(CART));
    assert_eq!(panel(&host).trash_confirm, None);
}

#[test]
fn remove_confirmation_escape_cancels() {
    let mut host = host();
    assert!(host
        .editor_state
        .editor_ui
        .projects_panel
        .begin_trash_confirm(CART));
    assert!(host.apply_escape(), "Escape is the question's");
    assert_eq!(panel(&host).trash_confirm, None);
    assert_eq!(panel(&host).trash_commit, None, "and never confirms");
    assert!(!host.apply_escape() || panel(&host).trash_commit.is_none());
}

#[test]
fn the_cancel_button_and_a_press_on_the_canvas_end_the_question() {
    let mut host = host();
    {
        let at = trash_button(&host, 0);
        press_at(&mut host, at)
    };
    {
        let at = cancel_button(&host, 0);
        press_at(&mut host, at)
    };
    assert_eq!(panel(&host).trash_confirm, None, "the cross cancels");
    assert_eq!(panel(&host).trash_commit, None);

    {
        let at = trash_button(&host, 1);
        press_at(&mut host, at)
    };
    assert!(panel(&host).trash_confirm.is_some());
    press_at(&mut host, Point2D::new(600.0, 300.0));
    assert_eq!(
        panel(&host).trash_confirm,
        None,
        "a press elsewhere cancels"
    );
    assert_eq!(panel(&host).trash_commit, None);
}

#[test]
fn a_design_started_from_the_host_takes_the_keyboard_for_its_rename() {
    let mut host = host();
    assert!(host.begin_project_rename(CART));
    assert!(host.project_rename_active(), "the rename owns the keyboard");
    assert!(!host.begin_project_rename("/work/shop/designs/none.op"));
}
