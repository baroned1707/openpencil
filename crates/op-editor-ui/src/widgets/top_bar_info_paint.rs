//! Paint of the info bar's items (the Git button stays with `paint_chrome`,
//! which already owns its hover and glyph).
//!
//! Flat, borderless and square like the rest of the Figma UI3 chrome: the
//! breadcrumb parts and the save label are plain muted text with the file name
//! in the foreground colour, and the unsaved mark is a filled dot.

use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_panel::status_color;
use crate::widgets::top_bar::*;
use crate::widgets::top_bar_info::{
    BreadcrumbCell, ChipCell, InfoLayout, SaveCell, AGENT_DOT, CHIP_PAD_X, META_FONT, NAME_FONT,
    PAGE_CHEVRON, UNSAVED_DOT,
};
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};
use op_editor_core::SaveState;
use op_editor_core::{DocumentTabStatus, TopBarButton};

impl TopBar {
    /// Paint every item of `layout` on the bar's centre line.
    pub(super) fn paint_info_items(
        &self,
        cx: &mut PaintCx<'_>,
        layout: &InfoLayout,
        center_y: f32,
    ) {
        if let Some(breadcrumb) = &layout.breadcrumb {
            self.paint_breadcrumb(cx, breadcrumb, center_y);
        }
        if let Some(save) = &layout.save {
            self.paint_save_marker(cx, save, center_y);
        }
        if let Some(page) = &layout.page {
            self.paint_page_chip(cx, page, center_y);
        }
        if let Some(agent) = &layout.agent {
            self.paint_agent_item(cx, agent, center_y);
        }
        if let Some(unsaved) = &layout.unsaved {
            self.paint_unsaved_chip(cx, unsaved, center_y);
        }
    }

    /// The current page and a chevron. Pressed while its menu is open.
    fn paint_page_chip(&self, cx: &mut PaintCx<'_>, cell: &ChipCell, center_y: f32) {
        let color = paint_hover_bg(
            cx,
            &self.theme,
            cell.rect,
            self.is_hovered(TopBarButton::PageChip),
            self.is_pressed(TopBarButton::PageChip) || self.page_menu_open,
        );
        let label = cell.label.as_deref().unwrap_or("");
        let run = TextLayout::single_run(
            label,
            "system-ui",
            META_FONT,
            color.to_jian(),
            Point2D::ZERO,
        );
        cx.backend.draw_text(
            &run,
            Point2D::new(cell.rect.origin.x + CHIP_PAD_X, center_y + 4.0),
        );
        draw_icon(
            cx.backend,
            Icon::ChevronDown,
            Point2D::new(
                cell.rect.origin.x + cell.rect.size.x - CHIP_PAD_X - PAGE_CHEVRON,
                center_y - PAGE_CHEVRON / 2.0,
            ),
            PAGE_CHEVRON,
            color,
            1.5,
        );
    }

    /// A dot in the running colour and, while there is room, what is running
    /// where. A flat chip: only the hover wash shows its edge.
    fn paint_agent_item(&self, cx: &mut PaintCx<'_>, cell: &ChipCell, center_y: f32) {
        let color = paint_hover_bg(
            cx,
            &self.theme,
            cell.rect,
            self.is_hovered(TopBarButton::AgentActivity),
            self.is_pressed(TopBarButton::AgentActivity),
        );
        let dot = status_color(&self.theme, DocumentTabStatus::Running).unwrap_or(color);
        let dot_x = cell.rect.origin.x + CHIP_PAD_X;
        cx.backend.fill_round_rect(
            Rect::xywh(dot_x, center_y - AGENT_DOT / 2.0, AGENT_DOT, AGENT_DOT),
            AGENT_DOT / 2.0,
            dot,
        );
        if let Some(label) = &cell.label {
            let run = TextLayout::single_run(
                label,
                "system-ui",
                META_FONT,
                color.to_jian(),
                Point2D::ZERO,
            );
            cx.backend
                .draw_text(&run, Point2D::new(dot_x + AGENT_DOT + 6.0, center_y + 4.0));
        }
    }

    fn paint_unsaved_chip(&self, cx: &mut PaintCx<'_>, cell: &ChipCell, center_y: f32) {
        let color = paint_hover_bg(
            cx,
            &self.theme,
            cell.rect,
            self.is_hovered(TopBarButton::OtherUnsaved),
            self.is_pressed(TopBarButton::OtherUnsaved),
        );
        if let Some(label) = &cell.label {
            let run = TextLayout::single_run(
                label,
                "system-ui",
                META_FONT,
                color.to_jian(),
                Point2D::ZERO,
            );
            cx.backend.draw_text(
                &run,
                Point2D::new(cell.rect.origin.x + CHIP_PAD_X, center_y + 4.0),
            );
        }
    }

    fn paint_breadcrumb(&self, cx: &mut PaintCx<'_>, cell: &BreadcrumbCell, center_y: f32) {
        // A flat chip: the hover wash is the only sign it can be pressed.
        if cell.clickable {
            paint_hover_bg(
                cx,
                &self.theme,
                cell.rect,
                self.is_hovered(TopBarButton::Breadcrumb),
                self.is_pressed(TopBarButton::Breadcrumb),
            );
        }
        for part in &cell.parts {
            let (size, color, baseline) = if part.is_file_name {
                (NAME_FONT, self.theme.foreground, center_y + 5.0)
            } else {
                (META_FONT, self.theme.muted_foreground, center_y + 4.0)
            };
            let run = TextLayout::single_run(
                &part.text,
                "system-ui",
                size,
                color.to_jian(),
                Point2D::ZERO,
            );
            cx.backend.draw_text(&run, Point2D::new(part.x, baseline));
        }
    }

    fn paint_save_marker(&self, cx: &mut PaintCx<'_>, cell: &SaveCell, center_y: f32) {
        match cell.label {
            Some(label) => {
                let run = TextLayout::single_run(
                    label,
                    "system-ui",
                    META_FONT,
                    self.theme.muted_foreground.to_jian(),
                    Point2D::ZERO,
                );
                cx.backend
                    .draw_text(&run, Point2D::new(cell.rect.origin.x, center_y + 4.0));
            }
            None => {
                let color: Color = if self.info.save == SaveState::Missing {
                    self.theme.status_warning
                } else {
                    self.theme.foreground
                };
                cx.backend.fill_round_rect(
                    Rect::xywh(
                        cell.rect.origin.x,
                        center_y - UNSAVED_DOT / 2.0,
                        UNSAVED_DOT,
                        UNSAVED_DOT,
                    ),
                    UNSAVED_DOT / 2.0,
                    color,
                );
            }
        }
    }
}
