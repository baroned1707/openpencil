//! The Projects list's open mark: the Cmd+digit badge on an open file's row.
//!
//! The badge is the open mark and the shortcut hint in one. Its digit comes
//! from `shortcut_digit_for`, the function the Cmd+digit chord itself agrees
//! with, over the same open order (`EditorUiState.document_tabs`); a document
//! the chord cannot reach gets a plain dot. Same look as the rest of the list:
//! flat, borderless, theme tokens only.

use op_editor_core::host_keyboard_transitions::shortcut_digit_for;
use op_editor_core::DocumentId;

use super::{draw_label, ProjectsPanel};
use crate::theme::Theme;
use crate::widgets::projects_panel_layout::ROW_PAD_RIGHT;
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};

const BADGE_FONT: f32 = 11.0;
const BADGE_DOT: f32 = 4.0;

impl ProjectsPanel<'_> {
    /// The Cmd+digit of the open document `id`. `None` when `id` is not among
    /// the open tabs (a row published a frame ahead of the tab list);
    /// `Some(None)` when it is open but no chord reaches it (it sits past the
    /// eighth tab and is not the last, or only one document is open).
    pub(crate) fn open_digit(&self, id: DocumentId) -> Option<Option<u8>> {
        let index = self.tabs.iter().position(|tab| tab.id == id)?;
        Some(shortcut_digit_for(self.tabs.len(), index))
    }

    /// Paint the open mark of document `id` against the row's right edge:
    /// `⌘N`, or a dot when no digit reaches it. Returns the x of its left edge,
    /// so the label can stop short of it (the row's right edge when `id` is not
    /// among the open tabs and nothing is painted).
    pub(super) fn paint_open_mark(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row_rect: Rect,
        id: DocumentId,
    ) -> f32 {
        let right = row_rect.origin.x + row_rect.size.x - ROW_PAD_RIGHT;
        match self.open_digit(id) {
            None => right,
            Some(Some(digit)) => {
                let label = format!("\u{2318}{digit}");
                let width = text_metrics::measure_chrome(cx.backend, &label, BADGE_FONT);
                draw_label(
                    cx,
                    &label,
                    BADGE_FONT,
                    400,
                    theme.muted_foreground,
                    Point2D::new(
                        right - width,
                        jian_widgets::centered_text_baseline_y(row_rect, BADGE_FONT),
                    ),
                );
                right - width
            }
            Some(None) => {
                let left = right - BADGE_DOT;
                cx.backend.fill_round_rect(
                    Rect::xywh(
                        left,
                        row_rect.origin.y + (row_rect.size.y - BADGE_DOT) / 2.0,
                        BADGE_DOT,
                        BADGE_DOT,
                    ),
                    BADGE_DOT / 2.0,
                    theme.muted_foreground,
                );
                left
            }
        }
    }
}
