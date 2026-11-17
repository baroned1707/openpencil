//! The "Download missing fonts automatically" switch at the top of the
//! Settings Fonts tab. Only the desktop host (which can download) shows it:
//! `font_downloads.supported` is false everywhere else and the block takes no
//! room.

use crate::theme::Theme;
use crate::widgets::agent_settings_i18n::t as t_settings;
use crate::widgets::agent_settings_rows::{
    paint_row_label, row_control_rect, row_height, RowLines,
};
use crate::widgets::agent_settings_switch::{
    paint_settings_switch, SETTINGS_SWITCH_H, SETTINGS_SWITCH_W,
};
use crate::widgets::PaintCx;
use crate::{Point2D, Rect};
use op_editor_core::editor_ui_state::EditorUiState;

/// Space between the switch row and the "Missing fonts" section below it.
const BLOCK_GAP: f32 = 12.0;

fn shown(ui: &EditorUiState) -> bool {
    ui.font_downloads.supported && !ui.touch_chrome()
}

/// Vertical space the block adds above the Fonts tab's first section.
pub(super) fn block_height(ui: &EditorUiState) -> f32 {
    if shown(ui) {
        row_height(RowLines::Two) + BLOCK_GAP
    } else {
        0.0
    }
}

fn row_rect(content: Rect, top: f32) -> Rect {
    Rect {
        origin: Point2D::new(content.origin.x, top),
        size: Point2D::new(content.size.x, row_height(RowLines::Two)),
    }
}

fn switch_rect(content: Rect, top: f32) -> Rect {
    row_control_rect(row_rect(content, top), SETTINGS_SWITCH_W, SETTINGS_SWITCH_H)
}

/// Whether `scrolled` (content coordinates) is on the switch row's control.
pub(super) fn hit(content: Rect, top: f32, ui: &EditorUiState, scrolled: Point2D) -> bool {
    shown(ui) && switch_rect(content, top).contains(scrolled)
}

pub(super) fn paint(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    ui: &EditorUiState,
    content: Rect,
    top: f32,
) {
    if !shown(ui) {
        return;
    }
    paint_row_label(
        cx,
        theme,
        row_rect(content, top),
        t_settings(ui, "settings.fonts.autoDownload"),
        Some(t_settings(ui, "settings.fonts.autoDownloadHint")),
        SETTINGS_SWITCH_W + 16.0,
    );
    paint_settings_switch(
        cx,
        theme,
        switch_rect(content, top),
        ui.agent_settings.auto_download_missing_fonts,
    );
}
