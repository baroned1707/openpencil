//! Press, hover, wheel and Escape handling of the page chip's dropdown.
//!
//! Shared by the hosts through plain functions over `EditorState`: the host
//! supplies the chip's rect (`anchor`, from the same bar layout paint used) and
//! the window rect, and runs the platform tail (a re-fit of the viewport after
//! a switch). Choosing a page goes through `switch_page`, the helper the left
//! rail's page rows use too.

use op_editor_core::{switch_page, EditorState, PageSwitch};

use crate::widgets::page_menu::PageMenu;
use crate::{Point2D, Rect};
use jian_widgets::components::select::SelectHit;

/// What a press did to the page menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageMenuPress {
    /// The menu is closed: the press is not the menu's.
    NotOpen,
    /// A press on the menu's own chrome: keep it open and swallow the press.
    Swallowed,
    /// A press outside: the menu closed and the press is consumed.
    Dismissed,
    /// A page was chosen and the menu closed; the host re-fits the viewport
    /// when the outcome says so.
    Chose(PageSwitch),
}

/// Open the menu, or close it when it is open (a second press on the chip).
pub fn toggle(state: &mut EditorState) {
    if state.editor_ui.projects_panel.info_bar.close_page_menu() {
        return;
    }
    // The menu hangs over the other dropdowns; leave none open beneath it.
    op_editor_core::host_press_transitions::close_top_bar_dropdowns(&mut state.editor_ui);
    let active = state.page_summary().active;
    state
        .editor_ui
        .projects_panel
        .info_bar
        .open_page_menu(active);
}

/// A press while the menu may be open.
pub fn press(
    state: &mut EditorState,
    anchor: Option<Rect>,
    viewport: Rect,
    point: Point2D,
) -> PageMenuPress {
    let Some(menu) = PageMenu::for_editor(state) else {
        return PageMenuPress::NotOpen;
    };
    // The chip scrolled out of the bar (a resize): nothing to hang from.
    let Some(anchor) = anchor else {
        state.editor_ui.projects_panel.info_bar.close_page_menu();
        return PageMenuPress::Dismissed;
    };
    match menu.hit(anchor, viewport, point) {
        SelectHit::Row(index) => {
            state.editor_ui.projects_panel.info_bar.close_page_menu();
            PageMenuPress::Chose(switch_page(state, index))
        }
        SelectHit::Inside => PageMenuPress::Swallowed,
        SelectHit::Outside => {
            state.editor_ui.projects_panel.info_bar.close_page_menu();
            PageMenuPress::Dismissed
        }
    }
}

/// Track the cursor over the rows. `None` when the menu is closed. `Some(true)`
/// when the hovered row changed.
pub fn cursor_move(
    state: &mut EditorState,
    anchor: Option<Rect>,
    viewport: Rect,
    point: Point2D,
) -> Option<bool> {
    let menu = PageMenu::for_editor(state)?;
    let hover = anchor.and_then(|anchor| match menu.hit(anchor, viewport, point) {
        SelectHit::Row(index) => Some(index),
        SelectHit::Inside | SelectHit::Outside => None,
    });
    let slot = &mut state.editor_ui.projects_panel.info_bar.page_menu.hover;
    let changed = *slot != hover;
    *slot = hover;
    Some(changed)
}

/// Scroll the list by a wheel notch (`delta_y` positive scrolls toward the top,
/// as everywhere else). `None` when the menu is closed.
pub fn scroll(state: &mut EditorState, delta_y: f32) -> Option<bool> {
    if !state.editor_ui.projects_panel.info_bar.page_menu.open {
        return None;
    }
    let max = PageMenu::max_scroll(state.page_names().len());
    let menu = &mut state.editor_ui.projects_panel.info_bar.page_menu;
    let next = (menu.scroll.offset - delta_y).clamp(0.0, max);
    let changed = next != menu.scroll.offset || menu.hover.is_some();
    menu.scroll.offset = next;
    menu.hover = None;
    Some(changed)
}

/// Paint the menu above everything it hangs over (a no-op while closed).
pub fn paint(
    cx: &mut crate::widgets::PaintCx<'_>,
    state: &EditorState,
    anchor: Option<Rect>,
    viewport: Rect,
) {
    if let (Some(menu), Some(anchor)) = (PageMenu::for_editor(state), anchor) {
        menu.paint(cx, anchor, viewport);
    }
}

/// The menu's footprint, for the host's "is the pointer over a popup" checks.
/// `None` while closed or without an anchor.
pub fn popup_rect(state: &EditorState, anchor: Option<Rect>, viewport: Rect) -> Option<Rect> {
    Some(PageMenu::for_editor(state)?.popup_rect(anchor?, viewport))
}

/// Escape closes the menu. `true` when it was open.
pub fn escape(state: &mut EditorState) -> bool {
    state.editor_ui.projects_panel.info_bar.close_page_menu()
}

#[cfg(test)]
#[path = "page_menu_flow_tests.rs"]
mod tests;
