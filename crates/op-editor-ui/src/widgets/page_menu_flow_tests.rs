use super::*;
use crate::widgets::page_menu::PageMenu;
use op_editor_core::NodeId;

const VIEWPORT: Rect = Rect::xywh(0.0, 0.0, 1200.0, 800.0);
const ANCHOR: Rect = Rect::xywh(400.0, 6.0, 90.0, 28.0);

fn state(pages: usize) -> EditorState {
    let mut state = EditorState::default();
    state.editor_ui.document_tabs_supported = true;
    while state.page_count() < pages {
        state.add_page().expect("page added");
    }
    state.ui.active_page_index = 0;
    state
}

fn row_point(state: &EditorState, row: usize) -> Point2D {
    let menu = PageMenu::for_editor(state).expect("menu is open");
    let popup = menu.popup_rect(ANCHOR, VIEWPORT);
    let row_h = jian_widgets::Density::Desktop.row_height();
    Point2D::new(
        popup.origin.x + 20.0,
        popup.origin.y + row_h * (row as f32 + 0.5),
    )
}

#[test]
fn a_press_on_the_chip_opens_the_menu_and_a_second_one_closes_it() {
    let mut state = state(3);
    toggle(&mut state);
    assert!(state.editor_ui.projects_panel.info_bar.page_menu.open);
    toggle(&mut state);
    assert!(!state.editor_ui.projects_panel.info_bar.page_menu.open);
}

#[test]
fn opening_closes_every_dropdown_beneath_it() {
    let mut state = state(3);
    state.editor_ui.file_menu_open = true;
    state.editor_ui.account_menu_open = true;
    toggle(&mut state);
    assert!(!state.editor_ui.file_menu_open && !state.editor_ui.account_menu_open);
}

#[test]
fn choosing_a_row_switches_through_the_shared_helper_and_closes() {
    let mut state = state(3);
    state.set_single_selection(NodeId::new("n1"));
    toggle(&mut state);
    let point = row_point(&state, 2);
    assert_eq!(
        press(&mut state, Some(ANCHOR), VIEWPORT, point),
        PageMenuPress::Chose(PageSwitch::Refit)
    );
    assert_eq!(state.ui.active_page_index, 2);
    assert!(
        !state.selection.anchor.is_real(),
        "the selection is dropped"
    );
    assert!(!state.editor_ui.projects_panel.info_bar.page_menu.open);
}

#[test]
fn choosing_the_active_page_keeps_the_view() {
    let mut state = state(3);
    toggle(&mut state);
    let point = row_point(&state, 0);
    assert_eq!(
        press(&mut state, Some(ANCHOR), VIEWPORT, point),
        PageMenuPress::Chose(PageSwitch::Preserved)
    );
}

#[test]
fn a_press_outside_dismisses_and_a_press_while_closed_is_not_the_menus() {
    let mut state = state(3);
    assert_eq!(
        press(&mut state, Some(ANCHOR), VIEWPORT, Point2D::new(5.0, 500.0)),
        PageMenuPress::NotOpen
    );
    toggle(&mut state);
    assert_eq!(
        press(&mut state, Some(ANCHOR), VIEWPORT, Point2D::new(5.0, 500.0)),
        PageMenuPress::Dismissed
    );
    assert!(!state.editor_ui.projects_panel.info_bar.page_menu.open);
}

#[test]
fn a_press_with_the_chip_gone_dismisses() {
    let mut state = state(3);
    toggle(&mut state);
    assert_eq!(
        press(&mut state, None, VIEWPORT, Point2D::new(400.0, 60.0)),
        PageMenuPress::Dismissed
    );
}

#[test]
fn hover_follows_the_row_under_the_cursor() {
    let mut state = state(3);
    assert_eq!(
        cursor_move(&mut state, Some(ANCHOR), VIEWPORT, Point2D::new(1.0, 1.0)),
        None
    );
    toggle(&mut state);
    let over_row_one = row_point(&state, 1);
    assert_eq!(
        cursor_move(&mut state, Some(ANCHOR), VIEWPORT, over_row_one),
        Some(true)
    );
    assert_eq!(
        state.editor_ui.projects_panel.info_bar.page_menu.hover,
        Some(1)
    );
    assert_eq!(
        cursor_move(&mut state, Some(ANCHOR), VIEWPORT, over_row_one),
        Some(false)
    );
    assert_eq!(
        cursor_move(&mut state, Some(ANCHOR), VIEWPORT, Point2D::new(1.0, 700.0)),
        Some(true)
    );
    assert_eq!(
        state.editor_ui.projects_panel.info_bar.page_menu.hover,
        None
    );
}

#[test]
fn a_long_page_list_scrolls_and_stays_inside_its_range() {
    let mut state = state(20);
    assert_eq!(
        scroll(&mut state, -30.0),
        None,
        "closed: not the menu's wheel"
    );
    toggle(&mut state);
    let max = PageMenu::max_scroll(20);
    assert_eq!(scroll(&mut state, -10_000.0), Some(true));
    assert_eq!(
        state
            .editor_ui
            .projects_panel
            .info_bar
            .page_menu
            .scroll
            .offset,
        max
    );
    scroll(&mut state, 10_000.0);
    assert_eq!(
        state
            .editor_ui
            .projects_panel
            .info_bar
            .page_menu
            .scroll
            .offset,
        0.0
    );
    // The last page is reachable once scrolled to the end.
    scroll(&mut state, -10_000.0);
    let point = row_point(&state, 7);
    assert!(matches!(
        press(&mut state, Some(ANCHOR), VIEWPORT, point),
        PageMenuPress::Chose(_)
    ));
    assert_eq!(state.ui.active_page_index, 19);
}

#[test]
fn escape_closes_only_an_open_menu() {
    let mut state = state(3);
    assert!(!escape(&mut state));
    toggle(&mut state);
    assert!(escape(&mut state));
    assert!(!state.editor_ui.projects_panel.info_bar.page_menu.open);
}

#[test]
fn the_menu_lists_the_page_names_with_the_active_one_marked() {
    let mut state = state(3);
    state.ui.active_page_index = 1;
    toggle(&mut state);
    let menu = PageMenu::for_editor(&state).unwrap();
    let popup = menu.popup_rect(ANCHOR, VIEWPORT);
    assert!(
        popup.origin.y >= ANCHOR.origin.y + ANCHOR.size.y,
        "hangs under the chip"
    );
    assert_eq!(
        popup.size.y,
        3.0 * jian_widgets::Density::Desktop.row_height()
    );
}
