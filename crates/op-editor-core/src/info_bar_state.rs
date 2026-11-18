//! State of the TopBar info bar's own popups.
//!
//! Only the page chip's dropdown has any: it is the same jian `Select` state
//! the locale picker uses (open flag, hovered row, scroll). It lives inside
//! `ProjectsPanelState` because `EditorUiState` is at the line cap, and it is
//! app-level like the rest of the dock: the open menu is closed whenever the
//! document on screen changes.

use jian_widgets::components::select::SelectState;
use jian_widgets::Density;

/// Rows of the page menu that stay above the active page when it opens far
/// down a long list.
const LEADING_ROWS: usize = 3;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct InfoBarState {
    /// The page chip's dropdown.
    pub page_menu: SelectState,
}

impl InfoBarState {
    /// Open the page menu, scrolled so the active page (`active`) is in view.
    pub fn open_page_menu(&mut self, active: usize) {
        let mut menu = SelectState {
            open: true,
            ..SelectState::default()
        };
        menu.scroll.offset =
            active.saturating_sub(LEADING_ROWS) as f32 * Density::Desktop.row_height();
        self.page_menu = menu;
    }

    /// Close the page menu. `true` when it was open.
    pub fn close_page_menu(&mut self) -> bool {
        let was_open = self.page_menu.open;
        self.page_menu = SelectState::default();
        was_open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_scrolls_the_active_page_into_view_and_closing_resets() {
        let mut state = InfoBarState::default();
        assert!(
            !state.close_page_menu(),
            "closing a closed menu changes nothing"
        );
        state.open_page_menu(12);
        assert!(state.page_menu.open);
        assert!(state.page_menu.scroll.offset > 0.0);
        state.page_menu.hover = Some(4);
        assert!(state.close_page_menu());
        assert_eq!(state, InfoBarState::default());
    }

    #[test]
    fn a_page_near_the_top_opens_unscrolled() {
        let mut state = InfoBarState::default();
        state.open_page_menu(2);
        assert_eq!(state.page_menu.scroll.offset, 0.0);
    }
}
