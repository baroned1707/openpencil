//! The quick switcher (Cmd+K): a keyboard-first way to reach any open document
//! or any file of a listed project, state and row building only.
//!
//! It lives on the Projects dock's state (`ProjectsPanelState::switcher`):
//! app-level like the rest of the dock, so it never travels with a document.
//! While it is open it owns the keyboard (`quick_switcher_keyboard`).
//!
//! The rows are DERIVED, never stored: open documents first (in the Cmd+digit
//! order, with their digit), then the project files that are not open. Each
//! group is ranked by [`crate::quick_switcher_match`] (a stable sort, so ties
//! keep their order, and an empty query keeps the dock's order), and the list
//! is capped at [`MAX_ROWS`]. `.fig` files are left out: opening one would run
//! the whole-file Figma import, which is the dock's job.

use jian_core::text_input::TextInputState;

use crate::document_tabs::{DocumentId, DocumentTabView};
use crate::host_keyboard_transitions::shortcut_digit_for;
use crate::project_tree::{ProjectFileKind, ProjectView};
use crate::quick_switcher_match::score_in_project;

/// The most rows the switcher lists.
pub const MAX_ROWS: usize = 50;
/// How many rows the popup shows at once; the list scrolls past this.
pub const VISIBLE_ROWS: usize = 8;

/// What Enter on a row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickSwitcherTarget {
    /// Show this open document.
    Document(DocumentId),
    /// Open the file at this path in a new document.
    File(String),
}

/// One row of the switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSwitcherRow {
    pub target: QuickSwitcherTarget,
    pub label: String,
    /// The project the file belongs to, shown muted; empty when it has none.
    pub project: String,
    /// An open document with unsaved changes.
    pub dirty: bool,
    /// The row is an open document.
    pub open: bool,
    /// The Cmd+digit that reaches this open document (`None` when none does).
    pub digit: Option<u8>,
}

/// The switcher's own state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuickSwitcherState {
    pub open: bool,
    /// The query being typed.
    pub input: TextInputState,
    /// The highlighted row (an index into the rows).
    pub selected: usize,
    /// The first row the popup shows, so the list scrolls with the selection.
    pub first_visible: usize,
}

impl QuickSwitcherState {
    /// Open with an empty query and the first row selected.
    pub fn open_fresh(&mut self, now_ms: u64) {
        self.open = true;
        self.input = TextInputState::default();
        self.input.touch(now_ms);
        self.selected = 0;
        self.first_visible = 0;
    }

    /// Close and forget the query. `true` when it was open.
    pub fn close(&mut self) -> bool {
        let was_open = self.open;
        *self = Self::default();
        was_open
    }

    /// The query changed: the old selection points at a different row now.
    pub fn query_changed(&mut self) {
        self.selected = 0;
        self.first_visible = 0;
    }

    /// Select `index` (clamped to `row_count`) and scroll it into view.
    pub fn select(&mut self, index: usize, row_count: usize) {
        if row_count == 0 {
            self.selected = 0;
            self.first_visible = 0;
            return;
        }
        self.selected = index.min(row_count - 1);
        self.scroll_to_selected(row_count);
    }

    /// Move the selection by `delta` rows, wrapping at both ends.
    pub fn move_selection(&mut self, delta: isize, row_count: usize) {
        if row_count == 0 {
            return;
        }
        let next = (self.selected as isize + delta).rem_euclid(row_count as isize);
        self.select(next as usize, row_count);
    }

    /// Scroll the popup by `rows` (positive = down) without moving the selection.
    pub fn scroll_rows(&mut self, rows: isize, row_count: usize) {
        let furthest = row_count.saturating_sub(VISIBLE_ROWS) as isize;
        self.first_visible = (self.first_visible as isize + rows).clamp(0, furthest) as usize;
    }

    fn scroll_to_selected(&mut self, row_count: usize) {
        if self.selected < self.first_visible {
            self.first_visible = self.selected;
        } else if self.selected >= self.first_visible + VISIBLE_ROWS {
            self.first_visible = self.selected + 1 - VISIBLE_ROWS;
        }
        self.first_visible = self
            .first_visible
            .min(row_count.saturating_sub(VISIBLE_ROWS));
    }

    /// The rows for the current query.
    pub fn rows(
        &self,
        tabs: &[DocumentTabView],
        projects: &[ProjectView],
    ) -> Vec<QuickSwitcherRow> {
        build_rows(self.input.text(), tabs, projects)
    }
}

/// The switcher's rows for `query`: see the module docs for the order.
pub fn build_rows(
    query: &str,
    tabs: &[DocumentTabView],
    projects: &[ProjectView],
) -> Vec<QuickSwitcherRow> {
    let mut open_rows: Vec<_> = tabs
        .iter()
        .enumerate()
        .filter_map(|(index, tab)| {
            let rank = score_in_project(query, &tab.label, &tab.project)?;
            Some((
                rank,
                QuickSwitcherRow {
                    target: QuickSwitcherTarget::Document(tab.id),
                    label: tab.label.clone(),
                    project: tab.project.clone(),
                    dirty: tab.dirty,
                    open: true,
                    digit: shortcut_digit_for(tabs.len(), index),
                },
            ))
        })
        .collect();
    let mut file_rows: Vec<_> = projects
        .iter()
        .flat_map(|project| project.files.iter().map(move |file| (project, file)))
        .filter(|(_, file)| file.open_id.is_none() && file.kind == ProjectFileKind::Design)
        .filter_map(|(project, file)| {
            let rank = score_in_project(query, &file.label, &project.name)?;
            Some((
                rank,
                QuickSwitcherRow {
                    target: QuickSwitcherTarget::File(file.path.clone()),
                    label: file.label.clone(),
                    project: project.name.clone(),
                    dirty: false,
                    open: false,
                    digit: None,
                },
            ))
        })
        .collect();
    // Stable: equal ranks keep the open order / the dock's order.
    open_rows.sort_by_key(|(rank, _)| *rank);
    file_rows.sort_by_key(|(rank, _)| *rank);
    open_rows
        .into_iter()
        .chain(file_rows)
        .map(|(_, row)| row)
        .take(MAX_ROWS)
        .collect()
}

#[cfg(test)]
#[path = "quick_switcher_tests.rs"]
mod tests;
