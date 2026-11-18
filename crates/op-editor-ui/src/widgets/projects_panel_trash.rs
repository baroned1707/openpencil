//! The Projects list's file-row states that come from the disk and from
//! removing a file: the "Move to Trash?" question and the "not on disk" marker.
//!
//! Same look as the rest of the list: flat, borderless, theme tokens only. The
//! question reuses the row's two button slots (trash, then pencil), so paint
//! and hit-test agree without a second geometry: the trash slot becomes the
//! confirm button and the pencil slot the cancel button.

use op_editor_core::ProjectRowTarget;

use super::{draw_label, paint_header_button, ProjectsPanel, FILE_FONT, FILE_INDENT};
use super::{FOLDER_FILE_INDENT, NAME_GAP, STATUS_SLOT};
use crate::theme::Theme;
use crate::widgets::icons::Icon;
use crate::widgets::projects_panel_layout::{ProjectRow, ROW_PAD_RIGHT};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::Point2D;

const MARKER_FONT: f32 = 11.0;

impl ProjectsPanel<'_> {
    /// A file row asking "Move to Trash?": the question where the name was,
    /// a confirm (check) and a cancel (cross) button at the row's right edge.
    pub(super) fn paint_trash_confirm_row(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        project: usize,
        index: usize,
    ) {
        let rect = row.rect;
        cx.backend.fill_rect(rect, theme.accent);
        let folder = self
            .projects
            .get(project)
            .and_then(|view| view.files.get(index))
            .is_some_and(|file| !file.folder.is_empty());
        let label_x = rect.origin.x
            + FILE_INDENT
            + if folder { FOLDER_FILE_INDENT } else { 0.0 }
            + STATUS_SLOT;
        let buttons_left = row
            .trash
            .or(row.rename)
            .map_or(rect.origin.x + rect.size.x - ROW_PAD_RIGHT, |button| {
                button.origin.x
            });
        let label = text_metrics::fit_chrome(
            cx.backend,
            self.text.trash_confirm,
            (buttons_left - NAME_GAP - label_x).max(0.0),
            FILE_FONT,
        );
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            theme.foreground,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FILE_FONT),
            ),
        );
        if let Some(button) = row.trash {
            let hot = self.hover == Some(ProjectRowTarget::TrashFile(project, index));
            paint_confirm_button(cx, theme, button, Icon::Check, hot);
        }
        if let Some(button) = row.rename {
            let hot = self.hover == Some(ProjectRowTarget::RenameFile(project, index));
            paint_header_button(cx, theme, button, Icon::Close, hot);
        }
    }

    /// The muted "not on disk" marker ending at `right` (the row's right edge,
    /// or the left of an open mark beside it). Returns the x of its left edge,
    /// so the label can stop short of it.
    pub(super) fn paint_not_on_disk(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        right: f32,
    ) -> f32 {
        let rect = row.rect;
        let width = text_metrics::measure_chrome(cx.backend, self.text.not_on_disk, MARKER_FONT);
        let left = right - width;
        draw_label(
            cx,
            self.text.not_on_disk,
            MARKER_FONT,
            400,
            theme.muted_foreground,
            Point2D::new(
                left,
                jian_widgets::centered_text_baseline_y(rect, MARKER_FONT),
            ),
        );
        left
    }
}

/// The question's confirm button: a flat wash and a destructive-toned glyph, so
/// the one irreversible-looking button reads differently from its cancel twin.
fn paint_confirm_button(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    button: crate::Rect,
    icon: Icon,
    hot: bool,
) {
    if hot {
        cx.backend.fill_rect(button, theme.button_hover);
    }
    crate::widgets::icons::draw_icon(
        cx.backend,
        icon,
        Point2D::new(
            button.origin.x + (button.size.x - super::PLUS_GLYPH) / 2.0,
            button.origin.y + (button.size.y - super::PLUS_GLYPH) / 2.0,
        ),
        super::PLUS_GLYPH,
        theme.destructive,
        1.6,
    );
}
