//! Shared missing-font modal used by native and web hosts.

use crate::theme::Theme;
use crate::widgets::editor_state_ext::{theme_for, translate};
use crate::widgets::file_menu::popup;
pub(crate) use crate::widgets::missing_fonts_row::{
    fit_button_width, paint_missing_font_row, paint_text, row_button_rect,
};
use crate::widgets::missing_fonts_row::{row_download_rect, BUTTON_HEIGHT};
use crate::widgets::property_panel_typography::{
    font_picker_action_in_layout, font_picker_entries, font_picker_hit_in_layout,
    font_picker_import_hover_in_layout, font_picker_layout_at, paint_font_picker_at,
    FontPickerAction, FontPickerEntry, FontPickerLayout,
};
use crate::widgets::{LayoutBox, LayoutCx, PaintCx, Widget, WidgetId};
use crate::{Point2D, Rect};
use jian_widgets::centered_text_baseline_y;
use op_editor_core::missing_fonts::MissingFontsPrompt;
use op_editor_core::{EditorState, EditorUiState};

const PANEL_WIDTH: f32 = 480.0;
const BASE_HEIGHT: f32 = 140.0;
const VIEWPORT_MARGIN: f32 = 8.0;
pub(crate) const ROW_HEIGHT: f32 = 72.0;
const ROWS_TOP: f32 = 68.0;
const HORIZONTAL_PAD: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingFontsHit {
    ChooseFont(usize),
    SelectFont(usize),
    ImportFont(usize),
    /// Per-row Download / Retry pill.
    DownloadFont(usize),
    /// Footer Download all button.
    DownloadAll,
    ClosePicker,
    PickerInside,
    Dismiss,
    Inside,
    Outside,
}

pub struct MissingFontsPanel<'a> {
    id: WidgetId,
    theme: Theme,
    prompt: &'a MissingFontsPrompt,
    ui: &'a EditorUiState,
}

impl<'a> MissingFontsPanel<'a> {
    pub fn for_editor(state: &'a EditorState) -> Option<Self> {
        let prompt = state.editor_ui.missing_fonts_prompt.as_ref()?;
        if !state.editor_ui.missing_fonts_modal_open || prompt.entries.is_empty() {
            return None;
        }
        Some(Self {
            id: WidgetId::new(5460),
            theme: theme_for(&state.editor_ui),
            prompt,
            ui: &state.editor_ui,
        })
    }

    pub fn rect(&self, viewport_w: f32, viewport_h: f32) -> Rect {
        let top = crate::widgets::TOP_BAR_HEIGHT + VIEWPORT_MARGIN;
        let max_height = (viewport_h - top - VIEWPORT_MARGIN).max(1.0);
        let natural_height = BASE_HEIGHT + ROW_HEIGHT * self.prompt.entries.len() as f32;
        let height = natural_height.min(max_height);
        let width = PANEL_WIDTH.min((viewport_w - VIEWPORT_MARGIN * 2.0).max(1.0));
        let x = ((viewport_w - width) / 2.0).max(VIEWPORT_MARGIN);
        let y = ((viewport_h - height) / 2.0).max(top);
        Rect {
            origin: Point2D::new(x, y),
            size: Point2D::new(width, height),
        }
    }

    /// Scrollable rows region; the header and footer remain fixed.
    pub fn rows_rect(&self, panel: Rect) -> Rect {
        Rect {
            origin: Point2D::new(panel.origin.x, panel.origin.y + ROWS_TOP),
            size: Point2D::new(panel.size.x, (panel.size.y - BASE_HEIGHT).max(0.0)),
        }
    }

    pub fn max_rows_scroll(&self, panel: Rect) -> f32 {
        (ROW_HEIGHT * self.prompt.entries.len() as f32 - self.rows_rect(panel).size.y).max(0.0)
    }

    fn rows_scroll(&self, panel: Rect) -> f32 {
        self.ui
            .missing_fonts_scroll
            .offset
            .clamp(0.0, self.max_rows_scroll(panel))
    }

    fn picker_row(&self) -> Option<usize> {
        match self.ui.font_picker_purpose {
            Some(op_editor_core::FontPickerPurpose::MissingFont {
                row,
                surface: op_editor_core::MissingFontSurface::Prompt,
            }) if self.ui.font_picker.open => Some(row),
            _ => None,
        }
    }

    pub fn picker_entries(&self) -> std::rc::Rc<Vec<FontPickerEntry>> {
        font_picker_entries(
            &self.ui.imported_font_families,
            &self.ui.bundled_font_families,
            &self.ui.system_font_families,
            &self.ui.font_picker_search,
        )
    }

    pub fn picker_layout(&self, panel: Rect, viewport: Rect) -> Option<FontPickerLayout> {
        let row = self.picker_row()?;
        let entry = self.prompt.entries.get(row)?;
        if entry.resolved {
            return None;
        }
        let entries = self.picker_entries();
        Some(font_picker_layout_at(
            self.row_button_rect(panel, row),
            300.0,
            inset_rect(viewport, 8.0),
            &entries,
            self.ui.font_import_supported,
            false,
            self.ui.font_picker.scroll.offset,
        ))
    }

    pub fn hit_test(&self, panel: Rect, viewport: Rect, point: Point2D) -> MissingFontsHit {
        if let Some(row) = self.picker_row() {
            if let Some(layout) = self.picker_layout(panel, viewport) {
                if let Some(action) = font_picker_action_in_layout(&layout, point) {
                    return match action {
                        FontPickerAction::Select(index) => MissingFontsHit::SelectFont(index),
                        FontPickerAction::Import => MissingFontsHit::ImportFont(row),
                        FontPickerAction::Remove(_) => MissingFontsHit::PickerInside,
                    };
                }
                return match font_picker_hit_in_layout(&layout, point) {
                    jian_widgets::components::select::SelectHit::Outside => {
                        MissingFontsHit::ClosePicker
                    }
                    _ => MissingFontsHit::PickerInside,
                };
            }
            return MissingFontsHit::ClosePicker;
        }
        if !panel.contains(point) {
            return MissingFontsHit::Outside;
        }

        let rows = self.rows_rect(panel);
        for (row, entry) in self.prompt.entries.iter().enumerate() {
            if rows.contains(point)
                && !entry.resolved
                && self.row_button_rect(panel, row).contains(point)
            {
                return MissingFontsHit::ChooseFont(row);
            }
            if rows.contains(point)
                && self
                    .download_pill_rect(panel, row)
                    .is_some_and(|rect| rect.contains(point))
            {
                return MissingFontsHit::DownloadFont(row);
            }
        }

        if self
            .download_all_rect(panel)
            .is_some_and(|rect| rect.contains(point))
        {
            MissingFontsHit::DownloadAll
        } else if dismiss_rect(panel, self.ui).contains(point) {
            MissingFontsHit::Dismiss
        } else {
            MissingFontsHit::Inside
        }
    }

    pub(crate) fn row_button_rect(&self, panel: Rect, row: usize) -> Rect {
        row_button_rect(modal_row_rect(panel, row, self.rows_scroll(panel)), self.ui)
    }

    pub(crate) fn download_pill_rect(&self, panel: Rect, row: usize) -> Option<Rect> {
        let entry = self.prompt.entries.get(row)?;
        row_download_rect(
            modal_row_rect(panel, row, self.rows_scroll(panel)),
            self.ui,
            entry,
        )
    }

    /// Footer button that starts every row that can still be downloaded; only
    /// worth showing when it saves a click (two or more rows).
    pub fn download_all_rect(&self, panel: Rect) -> Option<Rect> {
        if self
            .ui
            .font_downloads
            .downloadable_rows(&self.prompt.entries)
            < 2
        {
            return None;
        }
        let dismiss = dismiss_rect(panel, self.ui);
        let width = fit_button_width(translate(self.ui, "missingFonts.downloadAll"), 12.0);
        Some(Rect {
            origin: Point2D::new(dismiss.origin.x - 8.0 - width, dismiss.origin.y),
            size: Point2D::new(width, BUTTON_HEIGHT),
        })
    }

    pub fn picker_hover(
        &self,
        panel: Rect,
        viewport: Rect,
        point: Point2D,
    ) -> (Option<usize>, bool) {
        let Some(layout) = self.picker_layout(panel, viewport) else {
            return (None, false);
        };
        let hover = match font_picker_hit_in_layout(&layout, point) {
            jian_widgets::components::select::SelectHit::Row(index) => Some(index),
            _ => None,
        };
        (hover, font_picker_import_hover_in_layout(&layout, point))
    }

    pub fn paint_picker(&self, cx: &mut PaintCx<'_>, panel: Rect, viewport: Rect, now_ms: u64) {
        let Some(row) = self.picker_row() else {
            return;
        };
        let Some(entry) = self.prompt.entries.get(row) else {
            return;
        };
        let entries = self.picker_entries();
        paint_font_picker_at(
            cx,
            &self.theme,
            self.row_button_rect(panel, row),
            300.0,
            inset_rect(viewport, 8.0),
            self.ui.effective_locale(),
            &entries,
            self.ui.font_import_supported,
            false,
            &self.ui.font_picker_search,
            &self.ui.font_picker,
            self.ui.font_picker_import_hover,
            &entry.family,
            now_ms,
        );
    }
}

fn inset_rect(rect: Rect, inset: f32) -> Rect {
    Rect {
        origin: Point2D::new(rect.origin.x + inset, rect.origin.y + inset),
        size: Point2D::new(
            (rect.size.x - inset * 2.0).max(1.0),
            (rect.size.y - inset * 2.0).max(1.0),
        ),
    }
}

fn modal_row_rect(panel: Rect, row: usize, scroll: f32) -> Rect {
    Rect {
        origin: Point2D::new(
            panel.origin.x + HORIZONTAL_PAD,
            panel.origin.y + ROWS_TOP + row as f32 * ROW_HEIGHT - scroll,
        ),
        size: Point2D::new(panel.size.x - HORIZONTAL_PAD * 2.0, ROW_HEIGHT),
    }
}

fn dismiss_rect(panel: Rect, ui: &EditorUiState) -> Rect {
    let width = fit_button_width(translate(ui, "missingFonts.dismiss"), 12.0);
    Rect {
        origin: Point2D::new(
            panel.origin.x + panel.size.x - HORIZONTAL_PAD - width,
            panel.origin.y + panel.size.y - 44.0,
        ),
        size: Point2D::new(width, BUTTON_HEIGHT),
    }
}

impl Widget for MissingFontsPanel<'_> {
    fn id(&self) -> WidgetId {
        self.id
    }

    fn layout(&self, _cx: &LayoutCx) -> LayoutBox {
        LayoutBox {
            rect: Rect {
                origin: Point2D::new(0.0, 0.0),
                size: Point2D::new(
                    PANEL_WIDTH,
                    BASE_HEIGHT + ROW_HEIGHT * self.prompt.entries.len() as f32,
                ),
            },
        }
    }

    fn paint(&self, cx: &mut PaintCx<'_>, panel: Rect) {
        popup::paint_popup_surface(cx.backend, &self.theme, panel, popup::DIALOG_RADIUS);

        paint_text(
            cx,
            translate(self.ui, "missingFonts.title"),
            Point2D::new(panel.origin.x + HORIZONTAL_PAD, panel.origin.y + 28.0),
            16.0,
            600,
            self.theme.foreground,
        );
        paint_text(
            cx,
            translate(self.ui, "missingFonts.subtitle"),
            Point2D::new(panel.origin.x + HORIZONTAL_PAD, panel.origin.y + 50.0),
            11.0,
            400,
            self.theme.muted_foreground,
        );

        let rows = self.rows_rect(panel);
        let scroll = self.rows_scroll(panel);
        cx.backend.save();
        cx.backend.clip_rect(rows);
        for (row, entry) in self.prompt.entries.iter().enumerate() {
            let row_rect = modal_row_rect(panel, row, scroll);
            if row_rect.origin.y + row_rect.size.y <= rows.origin.y
                || row_rect.origin.y >= rows.origin.y + rows.size.y
            {
                continue;
            }
            paint_missing_font_row(cx, &self.theme, self.ui, entry, row, row_rect, row > 0);
        }
        cx.backend.restore();

        if let Some(all) = self.download_all_rect(panel) {
            let hovered = self.ui.missing_fonts_hover
                == Some(op_editor_core::missing_fonts::MissingFontsHover::DownloadAll);
            cx.backend.fill_round_rect(
                all,
                6.0,
                if hovered {
                    crate::widgets::panel_control_metrics::mix(
                        self.theme.primary,
                        self.theme.foreground,
                        0.12,
                    )
                } else {
                    self.theme.primary
                },
            );
            paint_text(
                cx,
                translate(self.ui, "missingFonts.downloadAll"),
                Point2D::new(all.origin.x + 12.0, centered_text_baseline_y(all, 12.0)),
                12.0,
                500,
                self.theme.primary_foreground,
            );
        }

        let dismiss = dismiss_rect(panel, self.ui);
        let dismiss_hovered = self.ui.missing_fonts_hover
            == Some(op_editor_core::missing_fonts::MissingFontsHover::Dismiss);
        if dismiss_hovered {
            cx.backend.fill_round_rect(dismiss, 6.0, self.theme.accent);
        }
        paint_text(
            cx,
            translate(self.ui, "missingFonts.dismiss"),
            Point2D::new(
                dismiss.origin.x + 12.0,
                centered_text_baseline_y(dismiss, 12.0),
            ),
            12.0,
            500,
            if dismiss_hovered {
                self.theme.foreground
            } else {
                self.theme.muted_foreground
            },
        );
    }

    fn access_node(&self) -> accesskit::Node {
        let mut node = accesskit::Node::new(accesskit::Role::Dialog);
        node.set_label(translate(self.ui, "missingFonts.title"));
        node
    }
}

#[cfg(test)]
#[path = "missing_fonts_panel_tests.rs"]
mod tests;
