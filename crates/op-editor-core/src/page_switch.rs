//! Switching the active page — the one helper behind the left rail's page rows
//! and the info bar's page chip, so the two can never drift.
//!
//! A page switch does three things: make the page active, drop the selection
//! (its nodes belong to the page that just left), and, when the page really
//! changed, tell the host to re-fit the viewport on the new page's content
//! instead of keeping the previous page's pan and zoom.

use crate::state::EditorState;

/// What the host still owes after [`switch_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSwitch {
    /// A different page is now active: re-fit the viewport on it.
    Refit,
    /// The page was already active. The selection may have been cleared, but
    /// the user's canvas view must be kept.
    Preserved,
}

/// The pages of the document as the rail and the page chip list them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageSummary {
    /// Page names in document order.
    pub names: Vec<String>,
    /// Index of the active page in `names`.
    pub active: usize,
}

impl EditorState {
    /// Page names in document order. A single-page document (`pages` is
    /// `None` or empty) has one synthetic "Page 1".
    pub fn page_names(&self) -> Vec<String> {
        match self.doc.pages.as_ref() {
            Some(pages) if !pages.is_empty() => {
                pages.iter().map(|page| page.name.clone()).collect()
            }
            _ => vec!["Page 1".to_string()],
        }
    }

    /// The page list and which one is active.
    pub fn page_summary(&self) -> PageSummary {
        let names = self.page_names();
        let active = self.ui.active_page_index.min(names.len().saturating_sub(1));
        PageSummary { names, active }
    }
}

/// Make page `index` the active one and clear the selection. An out-of-range
/// index leaves the active page as it was (the selection is still cleared, as
/// a rail press always did).
pub fn switch_page(state: &mut EditorState, index: usize) -> PageSwitch {
    let before = state.ui.active_page_index;
    let page_changed = index != before && state.set_active_page(index);
    state.clear_selection();
    if page_changed {
        PageSwitch::Refit
    } else {
        PageSwitch::Preserved
    }
}

#[cfg(test)]
#[path = "page_switch_tests.rs"]
mod tests;
