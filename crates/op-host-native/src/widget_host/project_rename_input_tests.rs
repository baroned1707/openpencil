//! Native-host routing of the Projects dock's inline file rename: how it
//! starts from presses, that it owns the keyboard while open (so no key can
//! touch the document behind it), and how a press elsewhere settles it.
//!
//! Windows-gated like the other tests that solve layout: building the scene
//! runs `jian_skia::SkiaMeasure`, which aborts under Windows CI's DirectWrite.

#![cfg(all(test, not(target_os = "windows")))]

use crate::widget_host::WidgetHostNative;
use op_editor_core::{EditorState, ProjectFileView, ProjectView};
use op_editor_ui::widgets::projects_panel_layout::RowKind;
use op_editor_ui::Point2D;

const VW: f32 = 1_400.0;
const VH: f32 = 900.0;

const BOARD: &str = r##"{
    "version": "1.0.0",
    "children": [
        { "type": "frame", "id": "board", "name": "Board", "x": 0, "y": 0,
          "width": 4000, "height": 500,
          "fill": [{"type":"solid","color":"#ffffff"}], "children": [
            { "type": "rectangle", "id": "box", "name": "Box", "x": 10, "y": 10,
              "width": 80, "height": 80,
              "fill": [{"type":"solid","color":"#ff0000"}] }
          ] }
    ]
}"##;

const CART: &str = "/work/shop/designs/cart.op";

fn file(label: &str) -> ProjectFileView {
    ProjectFileView {
        path: format!("/work/shop/designs/{label}"),
        label: label.to_string(),
        open_id: None,
        dirty: false,
        status: op_editor_core::DocumentTabStatus::Idle,
        active: false,
        folder: "designs".to_string(),
        kind: op_editor_core::ProjectFileKind::Design,
    }
}

fn host() -> WidgetHostNative {
    let document = jian_ops_schema::load_str(BOARD)
        .expect("parse fixture")
        .value;
    let mut host = WidgetHostNative::new();
    host.install_imported_state(EditorState::from_document(document));
    let ui = &mut host.editor_state.editor_ui;
    ui.home.hide();
    ui.document_tabs_supported = true;
    ui.projects_panel.projects = vec![ProjectView {
        root: "/work/shop".to_string(),
        name: "shop".to_string(),
        files: vec![file("cart.op"), file("checkout.op")],
        ..ProjectView::default()
    }];
    host.last_viewport_w = VW;
    host.last_viewport_h = VH;
    host.editor_state.viewport = op_editor_core::Viewport::IDENTITY;
    host.mark_dirty();
    host
}

fn row_point(host: &WidgetHostNative, file: usize) -> Point2D {
    let layout = host.projects_dock_layout(VW, VH).expect("the dock shows");
    let list = layout.list.expect("the list shows");
    let rect = list
        .row(RowKind::File(0, file))
        .expect("row is laid out")
        .rect;
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0 - 30.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn press_at(host: &mut WidgetHostNative, point: Point2D, now_ms: u64) -> bool {
    host.set_now_ms(now_ms);
    host.apply_cursor_move(point.x, point.y);
    let consumed = host.apply_press(point.x, point.y, VW, VH);
    host.apply_release_with_viewport(VW, VH);
    consumed
}

fn start_rename(host: &mut WidgetHostNative) {
    assert!(host
        .editor_state
        .editor_ui
        .projects_panel
        .begin_rename(CART, 0));
}

fn draft(host: &WidgetHostNative) -> String {
    host.editor_state
        .editor_ui
        .projects_panel
        .rename
        .as_ref()
        .expect("a rename is open")
        .input
        .text()
        .to_string()
}

fn document_fingerprint(host: &WidgetHostNative) -> String {
    let state = &host.editor_state;
    format!(
        "{:?}|{:?}|{:?}|{}",
        state.doc.children,
        state.selection,
        state.tool,
        state.document_revision()
    )
}

#[test]
fn a_double_click_on_a_design_row_starts_the_rename() {
    let mut host = host();
    let at = row_point(&host, 0);
    press_at(&mut host, at, 10_000);
    assert!(!host.project_rename_active());
    press_at(&mut host, at, 10_200);
    assert!(host.project_rename_active());
    assert_eq!(draft(&host), "cart");
}

#[test]
fn keys_while_renaming_never_touch_the_document() {
    let mut host = host();
    host.editor_state.select_all_top_level();
    start_rename(&mut host);
    let before = document_fingerprint(&host);

    // Typing replaces the selected stem; Backspace and Delete edit the draft.
    for c in "r".chars() {
        assert!(host.apply_text(c), "a typed key is consumed");
    }
    assert_eq!(draft(&host), "r", "'r' was typed, not the rectangle tool");
    assert!(host.apply_backspace());
    assert_eq!(draft(&host), "");
    assert!(
        host.apply_backspace(),
        "Backspace on an empty draft is still the field's"
    );
    assert!(host.apply_delete());
    // Chords and navigation that would act on the selection are swallowed.
    host.apply_duplicate();
    assert!(!host.apply_undo());
    assert!(!host.apply_redo());
    assert!(!host.apply_group());
    assert!(host.apply_project_rename_caret(false, false));
    assert!(host.apply_project_rename_edge(true, false));

    assert_eq!(
        document_fingerprint(&host),
        before,
        "the document, its selection and its tool are exactly as they were"
    );
    assert!(host.project_rename_active(), "and the field is still open");
}

#[test]
fn nudge_and_select_all_do_not_reach_the_canvas_either() {
    let mut host = host();
    host.editor_state.select_all_top_level();
    start_rename(&mut host);
    let before = document_fingerprint(&host);
    host.apply_nudge(1.0, 0.0);
    assert_eq!(document_fingerprint(&host), before, "an arrow key");
    host.apply_select_all();
    assert_eq!(document_fingerprint(&host), before);
    assert_eq!(
        host.editor_state
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap()
            .input
            .highlight_range(),
        Some((0, 4)),
        "Cmd+A selected the name"
    );
}

#[test]
fn enter_commits_and_escape_cancels() {
    let mut host = host();
    start_rename(&mut host);
    host.apply_text('b');
    host.apply_text('a');
    assert!(host.apply_send());
    assert_eq!(
        host.editor_state
            .editor_ui
            .projects_panel
            .rename_commit
            .as_ref()
            .map(|request| request.new_stem.as_str()),
        Some("ba")
    );
    host.editor_state.editor_ui.projects_panel.rename_commit = None;
    assert!(host.apply_escape());
    assert!(!host.project_rename_active());
    assert!(host
        .editor_state
        .editor_ui
        .projects_panel
        .rename_commit
        .is_none());
}

#[test]
fn a_press_on_the_canvas_commits_a_valid_name_and_cancels_an_invalid_one() {
    let on_canvas = Point2D::new(600.0, 300.0);
    let mut host = host();
    start_rename(&mut host);
    host.apply_text('x');
    press_at(&mut host, on_canvas, 11_000);
    assert_eq!(
        host.editor_state
            .editor_ui
            .projects_panel
            .rename_commit
            .as_ref()
            .map(|request| request.new_stem.as_str()),
        Some("x"),
        "a valid name is kept"
    );

    let mut host = self::host();
    start_rename(&mut host);
    host.apply_text('a');
    host.apply_text('/');
    press_at(&mut host, on_canvas, 11_000);
    assert!(!host.project_rename_active(), "an invalid one is dropped");
    assert!(host
        .editor_state
        .editor_ui
        .projects_panel
        .rename_commit
        .is_none());
}

#[test]
fn an_ime_commit_lands_in_the_field() {
    let mut host = host();
    start_rename(&mut host);
    assert!(host.apply_ime_commit("星图"));
    assert_eq!(draft(&host), "星图");
    host.apply_ime_preedit("x", None);
    assert_eq!(draft(&host), "星图", "a preedit does not edit the draft");
}

#[test]
fn starting_a_rename_blurs_the_other_inputs() {
    let mut host = host();
    host.editor_state.chat.focused = true;
    let at = row_point(&host, 1);
    press_at(&mut host, at, 20_000);
    press_at(&mut host, at, 20_100);
    assert!(host.project_rename_active());
    assert!(
        !host.editor_state.chat.focused,
        "the chat input gave up the keyboard"
    );
    assert_eq!(
        host.editor_state
            .editor_ui
            .projects_panel
            .rename
            .as_ref()
            .unwrap()
            .path,
        "/work/shop/designs/checkout.op"
    );
}

#[test]
fn the_caret_blinks_through_the_shared_input_resolver() {
    let mut host = host();
    start_rename(&mut host);
    assert!(host.editor_state.active_text_input().is_some());
    assert!(host.input_active_pub());
}
