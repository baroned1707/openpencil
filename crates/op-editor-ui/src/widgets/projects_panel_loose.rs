//! The Projects list's "Open, not in a project" section: a label and one row
//! per open document no project lists (untitled ones, files outside every
//! listed project). Each row has the status dot, the label, the unsaved bullet
//! and, at the right edge, the open mark (`projects_panel_open`) or, hovered,
//! the close button. Same look as the rest of the list: flat, borderless,
//! theme tokens only.

use op_editor_core::{DocumentTabView, ProjectRowTarget};

use super::{
    draw_label, paint_header_button, status_color, ProjectsPanel, DIRTY_DOT, DIRTY_GAP, FILE_FONT,
    FILE_INDENT, NAME_GAP, SEMIBOLD, STATUS_DOT, STATUS_SLOT,
};
use crate::theme::Theme;
use crate::widgets::icons::Icon;
use crate::widgets::projects_panel_layout::{ProjectRow, ROW_PAD_RIGHT};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

impl ProjectsPanel<'_> {
    /// The label over the "Open, not in a project" section.
    pub(super) fn paint_loose_header(&self, cx: &mut PaintCx<'_>, theme: &Theme, row: &ProjectRow) {
        let rect = row.rect;
        // Where a project header's name starts, so the section lines up with them.
        let x = rect.origin.x + super::CHEVRON_PAD_LEFT + super::CHEVRON_SIZE + 4.0;
        let max = (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - x).max(0.0);
        let label = crate::util::ellipsize_to_width(self.text.open_no_project, max, |candidate| {
            text_metrics::measure_chrome_weighted(
                cx.backend,
                candidate,
                super::HEADER_FONT,
                SEMIBOLD,
            )
        });
        draw_label(
            cx,
            &label,
            super::HEADER_FONT,
            SEMIBOLD,
            theme.muted_foreground,
            Point2D::new(
                x,
                jian_widgets::centered_text_baseline_y(rect, super::HEADER_FONT),
            ),
        );
    }

    /// A row of the open-document section: status dot, label, unsaved bullet,
    /// and the open mark (or, hovered, the close button) at the right edge.
    pub(super) fn paint_loose_doc(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        tab_index: usize,
    ) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        let rect = row.rect;
        let hovered = matches!(
            self.hover,
            Some(
                ProjectRowTarget::OpenDocument(i) | ProjectRowTarget::CloseOpenDocument(i)
            ) if i == tab_index
        );
        paint_row_wash(cx, theme, rect, tab, hovered);
        paint_status_dot(cx, theme, rect, rect.origin.x + FILE_INDENT, tab);
        let label_x = rect.origin.x + FILE_INDENT + STATUS_SLOT;
        let bullet_span = if tab.dirty {
            DIRTY_GAP + DIRTY_DOT
        } else {
            0.0
        };
        let right_limit = match row.close.filter(|_| hovered) {
            Some(button) => button.origin.x - NAME_GAP,
            None => self.paint_open_mark(cx, theme, rect, tab.id) - NAME_GAP,
        };
        let max = (right_limit - label_x - bullet_span).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, &tab.label, max, FILE_FONT);
        let width = text_metrics::measure_chrome(cx.backend, &label, FILE_FONT);
        // Every row here is an open document, so every label is in the open tone
        // (as an open file's row is in a project).
        let color = theme.foreground;
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            color,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FILE_FONT),
            ),
        );
        if tab.dirty {
            paint_dirty_bullet(cx, rect, label_x + width + DIRTY_GAP, color);
        }
        if let Some(button) = row.close.filter(|_| hovered) {
            let hot = self.hover == Some(ProjectRowTarget::CloseOpenDocument(tab_index));
            paint_header_button(cx, theme, button, Icon::Close, hot);
        }
    }
}

/// The active / hovered wash of an open-document row.
fn paint_row_wash(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    tab: &DocumentTabView,
    hovered: bool,
) {
    if tab.active {
        cx.backend.fill_rect(rect, theme.row_selected_primary);
    } else if hovered {
        cx.backend.fill_rect(rect, theme.accent);
    }
}

/// The agent-status dot in a row's leading slot.
fn paint_status_dot(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    slot_x: f32,
    tab: &DocumentTabView,
) {
    let Some(color): Option<Color> = status_color(theme, tab.status) else {
        return;
    };
    cx.backend.fill_round_rect(
        Rect::xywh(
            slot_x + (STATUS_SLOT - STATUS_DOT) / 2.0 - 2.0,
            rect.origin.y + (rect.size.y - STATUS_DOT) / 2.0,
            STATUS_DOT,
            STATUS_DOT,
        ),
        STATUS_DOT / 2.0,
        color,
    );
}

/// The unsaved bullet after a label.
fn paint_dirty_bullet(cx: &mut PaintCx<'_>, rect: Rect, x: f32, color: Color) {
    cx.backend.fill_round_rect(
        Rect::xywh(
            x,
            rect.origin.y + (rect.size.y - DIRTY_DOT) / 2.0,
            DIRTY_DOT,
            DIRTY_DOT,
        ),
        DIRTY_DOT / 2.0,
        color,
    );
}
