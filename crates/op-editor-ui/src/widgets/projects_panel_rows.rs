//! The Projects list's secondary rows: folder sub-headers, status lines, the
//! empty state and the hover tooltips of the header buttons.
//!
//! Split from `projects_panel.rs` (header and file rows) to stay under the
//! file-size ceiling. Same look: flat, borderless, theme tokens only.

use op_editor_core::{ProjectRowTarget, ProjectScanState};

use super::{draw_label, paint_header_button, ProjectsPanel, FILE_FONT, FILE_INDENT};
use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_panel_layout::{
    ProjectRow, ProjectsPanelLayout, RowKind, ROW_PAD_RIGHT,
};
use crate::widgets::text_metrics;
use crate::widgets::tooltip::{paint_tooltip, TooltipPlacement};
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};

const FOLDER_FONT: f32 = 11.0;
const NOTE_FONT: f32 = 11.0;
const EMPTY_FONT: f32 = 12.0;
const EMPTY_PAD_TOP: f32 = 28.0;
const EMPTY_HINT_GAP: f32 = 22.0;
const EMPTY_SIDE_PAD: f32 = 16.0;
const FOLDER_CHEVRON: f32 = 12.0;
const FOLDER_GAP: f32 = 4.0;
const BUTTON_GLYPH: f32 = 14.0;
const BUTTON_GAP: f32 = 6.0;

impl ProjectsPanel<'_> {
    /// No project at all: a headline, a one-line hint and the "Open project"
    /// button.
    pub(super) fn paint_empty(
        &self,
        cx: &mut PaintCx<'_>,
        layout: &ProjectsPanelLayout,
        theme: &Theme,
    ) {
        let panel = layout.panel;
        let max = (panel.size.x - EMPTY_SIDE_PAD * 2.0).max(0.0);
        let centred = |cx: &mut PaintCx<'_>, text: &str, font: f32, color, top: f32| {
            let text = text_metrics::fit_chrome(cx.backend, text, max, font);
            let width = text_metrics::measure_chrome(cx.backend, &text, font);
            draw_label(
                cx,
                &text,
                font,
                400,
                color,
                Point2D::new(panel.origin.x + (panel.size.x - width) / 2.0, top),
            );
        };
        let top = panel.origin.y + EMPTY_PAD_TOP;
        centred(cx, self.text.empty, EMPTY_FONT, theme.foreground, top);
        centred(
            cx,
            self.text.open_hint,
            FOLDER_FONT,
            theme.muted_foreground,
            top + EMPTY_HINT_GAP,
        );
        let Some(button) = layout.empty_button else {
            return;
        };
        let hot = self.hover == Some(ProjectRowTarget::OpenProject);
        // The one call to action of an empty dock: a flat primary block, washed
        // the way every other button is when the pointer is on it.
        cx.backend.fill_rect(button, theme.primary);
        if hot {
            cx.backend.fill_rect(button, theme.button_hover);
        }
        let label = text_metrics::fit_chrome(
            cx.backend,
            trimmed(self.text.open_project),
            (button.size.x - BUTTON_GLYPH - BUTTON_GAP - EMPTY_SIDE_PAD).max(0.0),
            FILE_FONT,
        );
        let label_width = text_metrics::measure_chrome(cx.backend, &label, FILE_FONT);
        let content = BUTTON_GLYPH + BUTTON_GAP + label_width;
        let start = button.origin.x + (button.size.x - content) / 2.0;
        draw_icon(
            cx.backend,
            Icon::FolderPlus,
            Point2D::new(
                start,
                button.origin.y + (button.size.y - BUTTON_GLYPH) / 2.0,
            ),
            BUTTON_GLYPH,
            theme.primary_foreground,
            1.6,
        );
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            theme.primary_foreground,
            Point2D::new(
                start + BUTTON_GLYPH + BUTTON_GAP,
                jian_widgets::centered_text_baseline_y(button, FILE_FONT),
            ),
        );
    }

    /// A folder sub-header: a chevron and the folder's relative path (its tail
    /// when it is long), muted; a folded folder shows how many files it hides.
    pub(super) fn paint_folder(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        project: usize,
        first_file: usize,
    ) {
        let Some(view) = self.projects.get(project) else {
            return;
        };
        let Some(folder) = view.files.get(first_file).map(|file| file.folder.as_str()) else {
            return;
        };
        let rect = row.rect;
        let hovered = matches!(
            self.hover,
            Some(ProjectRowTarget::Folder(p, f) | ProjectRowTarget::NewHere(p, Some(f)))
                if (p, f) == (project, first_file)
        );
        if hovered {
            cx.backend.fill_rect(rect, theme.accent);
        }
        let folded = view.is_folder_collapsed(folder);
        draw_icon(
            cx.backend,
            if folded {
                Icon::ChevronRight
            } else {
                Icon::ChevronDown
            },
            Point2D::new(
                rect.origin.x + FILE_INDENT,
                rect.origin.y + (rect.size.y - FOLDER_CHEVRON) / 2.0,
            ),
            FOLDER_CHEVRON,
            theme.muted_foreground,
            1.6,
        );
        let right_edge = rect.origin.x + rect.size.x - ROW_PAD_RIGHT;
        let mut text_right = right_edge;
        // The hovered row's "New design here" button takes the count's place.
        let button = row.new_here.filter(|_| hovered);
        if let Some(button) = button {
            let hot = self.hover == Some(ProjectRowTarget::NewHere(project, Some(first_file)));
            paint_header_button(cx, theme, button, Icon::FilePlus, hot);
            text_right = button.origin.x - FOLDER_GAP;
        } else if folded {
            let hidden = view
                .files
                .iter()
                .filter(|file| file.folder == folder)
                .count();
            let count = hidden.to_string();
            let width = text_metrics::measure_chrome(cx.backend, &count, FOLDER_FONT);
            draw_label(
                cx,
                &count,
                FOLDER_FONT,
                400,
                theme.muted_foreground,
                Point2D::new(
                    right_edge - width,
                    jian_widgets::centered_text_baseline_y(rect, FOLDER_FONT),
                ),
            );
            text_right = right_edge - width - FOLDER_GAP * 2.0;
        }
        let label_x = rect.origin.x + FILE_INDENT + FOLDER_CHEVRON + FOLDER_GAP;
        let label = fit_tail(
            cx,
            &format!("{folder}/"),
            (text_right - label_x).max(0.0),
            FOLDER_FONT,
        );
        draw_label(
            cx,
            &label,
            FOLDER_FONT,
            400,
            theme.muted_foreground,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FOLDER_FONT),
            ),
        );
    }

    /// The line a project's scan state asks for: scanning, slow, unreadable or
    /// `N more not shown`.
    pub(super) fn paint_status(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        project: usize,
    ) {
        let Some(view) = self.projects.get(project) else {
            return;
        };
        let (text, warn) = match view.scan {
            ProjectScanState::Idle => return,
            ProjectScanState::Scanning => (self.text.scanning.to_string(), false),
            ProjectScanState::Slow => (self.text.slow.to_string(), true),
            ProjectScanState::Unreadable => (self.text.unreadable.to_string(), true),
            ProjectScanState::Truncated(omitted) => (
                self.text
                    .truncated
                    .replace("{{count}}", &omitted.to_string()),
                false,
            ),
        };
        let color = if warn {
            theme.status_warning
        } else {
            theme.muted_foreground
        };
        self.paint_note_in(cx, row, &text, color);
    }

    /// A muted one-line note at the file indent (nothing found here).
    pub(super) fn paint_note(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        text: &str,
    ) {
        self.paint_note_in(cx, row, text, theme.muted_foreground);
    }

    fn paint_note_in(
        &self,
        cx: &mut PaintCx<'_>,
        row: &ProjectRow,
        text: &str,
        color: crate::Color,
    ) {
        let rect = row.rect;
        let x = rect.origin.x + FILE_INDENT;
        let label = text_metrics::fit_chrome(
            cx.backend,
            text,
            (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - x).max(0.0),
            NOTE_FONT,
        );
        draw_label(
            cx,
            &label,
            NOTE_FONT,
            400,
            color,
            Point2D::new(x, jian_widgets::centered_text_baseline_y(rect, NOTE_FONT)),
        );
    }

    /// The tooltip of the header button under the pointer, painted above the
    /// rows (the buttons are icon-only).
    pub(super) fn paint_button_tooltip(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        layout: &ProjectsPanelLayout,
    ) {
        let asking = |project: usize, file: usize| {
            self.projects
                .get(project)
                .and_then(|view| view.files.get(file))
                .is_some_and(|view| self.trash_confirm == Some(view.path.as_str()))
        };
        let (kind, label) = match self.hover {
            Some(ProjectRowTarget::Close(project)) => (RowKind::Header(project), self.text.close),
            Some(ProjectRowTarget::Refresh(project)) => {
                (RowKind::Header(project), self.text.refresh)
            }
            // While a row asks "Move to Trash?" its pencil slot cancels.
            Some(ProjectRowTarget::RenameFile(project, file)) => (
                RowKind::File(project, file),
                if asking(project, file) {
                    self.text.cancel
                } else {
                    self.text.rename
                },
            ),
            Some(ProjectRowTarget::TrashFile(project, file)) => {
                (RowKind::File(project, file), self.text.trash)
            }
            Some(ProjectRowTarget::NewHere(project, None)) => {
                (RowKind::Header(project), self.text.new_here)
            }
            Some(ProjectRowTarget::NewHere(project, Some(file))) => {
                (RowKind::Folder(project, file), self.text.new_here)
            }
            _ => return,
        };
        let Some(row) = layout.row(kind) else {
            return;
        };
        let anchor = match self.hover {
            Some(ProjectRowTarget::Close(_)) => row.close,
            Some(ProjectRowTarget::RenameFile(..)) => row.rename,
            Some(ProjectRowTarget::TrashFile(..)) => row.trash,
            Some(ProjectRowTarget::NewHere(..)) => row.new_here,
            _ => row.refresh,
        };
        let Some(anchor) = anchor else { return };
        let band = (
            layout.panel.origin.x,
            layout.panel.origin.x + layout.panel.size.x,
        );
        paint_tooltip(
            cx,
            theme,
            Rect::xywh(
                anchor.origin.x,
                anchor.origin.y,
                anchor.size.x,
                anchor.size.y,
            ),
            trimmed(label),
            None,
            TooltipPlacement::Below,
            Some(band),
        );
    }
}

/// A label without the trailing ellipsis a menu-style string carries.
pub(crate) fn trimmed(text: &str) -> &str {
    text.trim_end_matches(['.', '…'])
}

/// Ellipsize at the START so the folder nearest the files stays readable.
fn fit_tail(cx: &mut PaintCx<'_>, text: &str, max_w: f32, font: f32) -> String {
    if text_metrics::measure_chrome(cx.backend, text, font) <= max_w {
        return text.to_string();
    }
    for (start, _) in text.char_indices().skip(1) {
        let candidate = format!("…{}", &text[start..]);
        if text_metrics::measure_chrome(cx.backend, &candidate, font) <= max_w {
            return candidate;
        }
    }
    "…".to_string()
}
