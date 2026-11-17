//! The Projects list's Figma rows: a `.fig` file with its conversion status,
//! the "Convert all (N)" action rows and their tooltips.
//!
//! Split from `projects_panel.rs` to stay under the file-size ceiling. Same
//! look: flat, borderless, theme tokens only. A `.fig` row leads with a small
//! filled `FIG` tag instead of the agent-status dot (a `.fig` is never open),
//! and carries its status as right-aligned text.

use op_editor_core::{FigStatus, ProjectFileKind, ProjectFileView, ProjectRowTarget};

use super::{draw_label, ProjectsPanel, FILE_FONT, FILE_INDENT};
use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_panel_layout::{
    ProjectRow, ProjectsPanelLayout, RowKind, ROW_PAD_RIGHT,
};
use crate::widgets::text_metrics;
use crate::widgets::tooltip::{paint_tooltip, TooltipPlacement};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

/// Indent of the files under a folder sub-header (the panel's own value).
const FOLDER_FILE_INDENT: f32 = 14.0;
const TAG_TEXT: &str = "FIG";
const TAG_FONT: f32 = 9.0;
const TAG_WIDTH: f32 = 22.0;
const TAG_HEIGHT: f32 = 12.0;
const TAG_GAP: f32 = 6.0;
const STATUS_FONT: f32 = 11.0;
/// The status text keeps at least this much room even beside a long name.
const STATUS_GAP: f32 = 8.0;
const ACTION_GLYPH: f32 = 14.0;
const ACTION_GAP: f32 = 6.0;

impl ProjectsPanel<'_> {
    /// A `.fig` row: tag, name, then its status at the right edge.
    pub(super) fn paint_fig_file(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        file: &ProjectFileView,
        project: usize,
        index: usize,
    ) {
        let ProjectFileKind::Figma(status) = &file.kind else {
            return;
        };
        let rect = row.rect;
        let hovered = self.hover == Some(ProjectRowTarget::File(project, index));
        if hovered {
            cx.backend.fill_rect(rect, theme.accent);
        }
        let name_color = if hovered {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        let left = rect.origin.x
            + FILE_INDENT
            + if file.folder.is_empty() {
                0.0
            } else {
                FOLDER_FILE_INDENT
            };
        cx.backend.fill_rect(
            Rect::xywh(
                left,
                rect.origin.y + (rect.size.y - TAG_HEIGHT) / 2.0,
                TAG_WIDTH,
                TAG_HEIGHT,
            ),
            theme.button_hover,
        );
        let tag_width = text_metrics::measure_chrome(cx.backend, TAG_TEXT, TAG_FONT);
        draw_label(
            cx,
            TAG_TEXT,
            TAG_FONT,
            600,
            theme.secondary_foreground,
            Point2D::new(
                left + (TAG_WIDTH - tag_width) / 2.0,
                jian_widgets::centered_text_baseline_y(rect, TAG_FONT),
            ),
        );

        let right_edge = rect.origin.x + rect.size.x - ROW_PAD_RIGHT;
        let (status_text, status_color) = self.fig_status(theme, status);
        let status_width = text_metrics::measure_chrome(cx.backend, status_text, STATUS_FONT);
        draw_label(
            cx,
            status_text,
            STATUS_FONT,
            400,
            status_color,
            Point2D::new(
                right_edge - status_width,
                jian_widgets::centered_text_baseline_y(rect, STATUS_FONT),
            ),
        );

        let label_x = left + TAG_WIDTH + TAG_GAP;
        let max = (right_edge - status_width - STATUS_GAP - label_x).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, &file.label, max, FILE_FONT);
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            name_color,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FILE_FONT),
            ),
        );
    }

    /// The status word of a `.fig` row and its colour.
    fn fig_status(&self, theme: &Theme, status: &FigStatus) -> (&str, Color) {
        match status {
            FigStatus::NotConverted => (self.text.fig_convert, theme.primary),
            FigStatus::Converted => (self.text.fig_converted, theme.muted_foreground),
            FigStatus::Converting => (self.text.fig_converting, theme.primary),
            FigStatus::Failed(_) => (self.text.fig_failed, theme.destructive),
        }
    }

    /// A "Convert all (N)" row: an arrow glyph and the label, in the primary
    /// colour (it is an action, not a file).
    pub(super) fn paint_convert_all(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        project: usize,
        folder: Option<usize>,
    ) {
        let Some(view) = self.projects.get(project) else {
            return;
        };
        let rect = row.rect;
        let (scope, indented) = match folder {
            Some(first) => (view.files.get(first).map(|file| file.folder.as_str()), true),
            None => (None, false),
        };
        let hovered = self.hover == Some(ProjectRowTarget::ConvertAll(project, folder));
        if hovered {
            cx.backend.fill_rect(rect, theme.accent);
        }
        let glyph_x = rect.origin.x + FILE_INDENT + if indented { FOLDER_FILE_INDENT } else { 0.0 };
        draw_icon(
            cx.backend,
            Icon::ArrowRight,
            Point2D::new(glyph_x, rect.origin.y + (rect.size.y - ACTION_GLYPH) / 2.0),
            ACTION_GLYPH,
            theme.primary,
            1.6,
        );
        let count = view.not_converted_count(scope);
        let text = self
            .text
            .fig_convert_all
            .replace("{{count}}", &count.to_string());
        let label_x = glyph_x + ACTION_GLYPH + ACTION_GAP;
        let max = (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - label_x).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, &text, max, FILE_FONT);
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            theme.primary,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FILE_FONT),
            ),
        );
    }

    /// The tooltip of the `.fig` row or "Convert all" row under the pointer.
    pub(super) fn paint_fig_tooltip(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        layout: &ProjectsPanelLayout,
    ) {
        let (kind, tip) = match self.hover {
            Some(ProjectRowTarget::File(project, index)) => {
                let Some(ProjectFileKind::Figma(status)) = self
                    .projects
                    .get(project)
                    .and_then(|view| view.files.get(index))
                    .map(|file| &file.kind)
                else {
                    return;
                };
                let tip = match status {
                    FigStatus::NotConverted => self.text.fig_convert_tip.to_string(),
                    FigStatus::Converted => self.text.fig_converted_tip.to_string(),
                    FigStatus::Converting => self.text.fig_converting.to_string(),
                    FigStatus::Failed(reason) => {
                        self.text.fig_failed_tip.replace("{{reason}}", reason)
                    }
                };
                (RowKind::File(project, index), tip)
            }
            Some(ProjectRowTarget::ConvertAll(project, folder)) => (
                RowKind::ConvertAll(project, folder),
                self.text.fig_convert_all_tip.to_string(),
            ),
            _ => return,
        };
        let Some(row) = layout.row(kind) else { return };
        let band = (
            layout.panel.origin.x,
            layout.panel.origin.x + layout.panel.size.x,
        );
        // Below the row unless that would leave the viewport.
        let below_fits = row.rect.origin.y + row.rect.size.y + 28.0
            <= layout.panel.origin.y + layout.panel.size.y;
        paint_tooltip(
            cx,
            theme,
            row.rect,
            &tip,
            None,
            if below_fits {
                TooltipPlacement::Below
            } else {
                TooltipPlacement::Above
            },
            Some(band),
        );
    }
}

#[cfg(test)]
#[path = "projects_fig_tests.rs"]
mod projects_fig_tests;
