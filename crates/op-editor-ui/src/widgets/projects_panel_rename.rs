//! The Projects list's inline rename: the field that replaces a file's label
//! while its name is being edited, and the one-line message under it.
//!
//! The field is a flat tone block (no outline) holding the shared
//! `TextInputView`, so the caret, the selection and the scrolling of a name
//! longer than the field behave like every other chrome input.

use op_editor_core::{ProjectFileView, ProjectRenameState, RenameNameError};

use super::{draw_label, ProjectsPanel, FILE_FONT, FILE_INDENT, FOLDER_FILE_INDENT, STATUS_SLOT};
use crate::theme::Theme;
use crate::widgets::projects_panel_layout::{
    ProjectRow, ProjectsPanelLayout, RowKind, ROW_PAD_RIGHT,
};
use crate::widgets::property_panel_text_input::paint_text_input_view;
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

/// Inner padding between the field's edge and its text.
const FIELD_PAD_X: f32 = 6.0;
/// Vertical inset of the field inside its row.
const FIELD_INSET_Y: f32 = 3.0;
/// Height of the message line under the field.
const MESSAGE_HEIGHT: f32 = 22.0;
const MESSAGE_FONT: f32 = 11.0;
const MESSAGE_INDENT: f32 = 8.0;

/// The inline field of a file row: from the label's left edge (less the
/// field's inner padding) to the row's right padding. Paint and hit-test both
/// read this, so a press inside the painted field is a press on the field.
pub(crate) fn rename_field_rect(row: &ProjectRow, file: &ProjectFileView) -> Rect {
    let label_x = row.rect.origin.x
        + FILE_INDENT
        + if file.folder.is_empty() {
            0.0
        } else {
            FOLDER_FILE_INDENT
        }
        + STATUS_SLOT;
    let left = label_x - FIELD_PAD_X;
    let right = row.rect.origin.x + row.rect.size.x - ROW_PAD_RIGHT;
    Rect::xywh(
        left,
        row.rect.origin.y + FIELD_INSET_Y,
        (right - left).max(0.0),
        (row.rect.size.y - FIELD_INSET_Y * 2.0).max(0.0),
    )
}

impl ProjectsPanel<'_> {
    /// A file row in edit mode: the row's wash, then the field.
    pub(super) fn paint_rename_row(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        file: &ProjectFileView,
        edit: &ProjectRenameState,
    ) {
        cx.backend.fill_rect(row.rect, theme.accent);
        let field = rename_field_rect(row, file);
        cx.backend.fill_rect(field, theme.background);
        paint_text_input_view(
            cx,
            theme,
            &edit.input,
            field,
            FILE_FONT,
            FIELD_PAD_X,
            jian_widgets::centered_text_baseline_y(field, FILE_FONT),
            self.now_ms,
            self.text.rename_placeholder,
            true,
        );
    }

    /// The message under the field being edited, over the row below it. Painted
    /// last so it covers that row; nothing in the layout moves.
    pub(super) fn paint_rename_error(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        layout: &ProjectsPanelLayout,
        surface: Color,
    ) {
        let Some(edit) = self.rename else { return };
        let Some(error) = edit.error else { return };
        let Some(row) = self.rename_row(layout, edit) else {
            return;
        };
        let rect = Rect::xywh(
            row.rect.origin.x,
            row.rect.origin.y + row.rect.size.y,
            row.rect.size.x,
            MESSAGE_HEIGHT,
        );
        cx.backend.fill_rect(rect, surface);
        let text = match error {
            RenameNameError::Taken => self.text.rename_exists,
            _ => self.text.rename_invalid,
        };
        // From the row's own left edge, not the label's: the dock is narrow and
        // the longest message ("A file with that name already exists") has to
        // fit on its one line.
        let left = rect.origin.x + MESSAGE_INDENT;
        let label = text_metrics::fit_chrome(
            cx.backend,
            text,
            (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - left).max(0.0),
            MESSAGE_FONT,
        );
        draw_label(
            cx,
            &label,
            MESSAGE_FONT,
            400,
            theme.destructive,
            Point2D::new(
                left,
                jian_widgets::centered_text_baseline_y(rect, MESSAGE_FONT),
            ),
        );
    }

    /// The laid-out row of the file being renamed.
    pub(super) fn rename_row<'l>(
        &self,
        layout: &'l ProjectsPanelLayout,
        edit: &ProjectRenameState,
    ) -> Option<&'l ProjectRow> {
        layout.rows.iter().find(|row| match row.kind {
            RowKind::File(project, file) => self
                .projects
                .get(project)
                .and_then(|view| view.files.get(file))
                .is_some_and(|view| view.path == edit.path),
            _ => false,
        })
    }
}
