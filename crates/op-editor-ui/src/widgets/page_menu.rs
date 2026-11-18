//! `PageMenu` — the dropdown behind the TopBar's page chip.
//!
//! Lists the document's pages with the active one marked, and hangs under the
//! chip. It is the same jian `Select` popup the locale picker uses (rows, hover
//! wash, check mark, scroll after eight rows), so a new dropdown pattern is not
//! introduced: only the rows and the anchor differ.

use crate::theme::Theme;
use crate::widgets::editor_state_ext::theme_for;
use crate::widgets::file_menu::popup;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};
use jian_widgets::components::select::{
    Select, SelectHit, SelectItem, SelectState, MAX_VISIBLE_ROWS,
};
use jian_widgets::{Density, Tokens};
use op_editor_core::EditorState;

pub struct PageMenu {
    theme: Theme,
    names: Vec<String>,
    active: usize,
    state: SelectState,
}

impl PageMenu {
    /// The menu while it is open. `None` when closed.
    pub fn for_editor(state: &EditorState) -> Option<Self> {
        let menu = &state.editor_ui.projects_panel.info_bar.page_menu;
        if !menu.open {
            return None;
        }
        let summary = state.page_summary();
        Some(Self {
            theme: theme_for(&state.editor_ui),
            names: summary.names,
            active: summary.active,
            state: menu.clone(),
        })
    }

    /// How far the list scrolls at most, in px.
    pub fn max_scroll(page_count: usize) -> f32 {
        page_count.saturating_sub(MAX_VISIBLE_ROWS) as f32 * Density::Desktop.row_height()
    }

    fn tokens(&self) -> Tokens {
        popup::borderless_tokens(&self.theme)
    }

    /// Where the menu hangs: under `anchor` (the page chip), kept inside
    /// `viewport`.
    pub fn popup_rect(&self, anchor: Rect, viewport: Rect) -> Rect {
        Select::popup_rect(anchor, viewport, self.names.len(), &self.tokens())
    }

    /// The row or area under `point`.
    pub fn hit(&self, anchor: Rect, viewport: Rect, point: Point2D) -> SelectHit {
        Select::hit(
            &self.state,
            anchor,
            viewport,
            self.names.len(),
            point,
            &self.tokens(),
        )
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, anchor: Rect, viewport: Rect) {
        let tokens = self.tokens();
        let popup_rect = self.popup_rect(anchor, viewport);
        popup::paint_soft_shadow(cx.backend, &self.theme, popup_rect, popup::SELECT_RADIUS);
        let items: Vec<SelectItem<'_>> = self
            .names
            .iter()
            .enumerate()
            .map(|(index, name)| SelectItem {
                label: name,
                selected: index == self.active,
                disabled: false,
            })
            .collect();
        Select {
            state: &self.state,
            items: &items,
        }
        .paint(cx.backend, anchor, viewport, &tokens);
    }
}
