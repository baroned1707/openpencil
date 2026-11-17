//! The Projects dock: a column at the window's far right edge that lists
//! every project and its design files.
//!
//! Figma UI3 look — flat, borderless, tone-separated. Open, it is a header
//! (folder glyph, title, an "Open project" button, a collapse chevron) over the
//! project list ([`ProjectsPanel`]); collapsed, it is a slim strip with a
//! chevron that re-opens it, a folder glyph and the project count. With no
//! project the strip swaps the count for a folder-plus button that opens the
//! folder picker, so a new user can find it.
//!
//! The widget only paints. Where its parts sit is [`ProjectsDockLayout`],
//! what a press means is `projects_dock_flow`.

use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_dock_layout::{ProjectsDockLayout, DOCK_TOGGLE_SIZE};
use crate::widgets::projects_panel::{draw_label, ProjectsPanel};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect};

const TITLE_FONT: f32 = 12.0;
const TITLE_WEIGHT: u16 = 600;
const HEADER_PAD_LEFT: f32 = 12.0;
const FOLDER_GLYPH: f32 = 14.0;
const TITLE_GAP: f32 = 8.0;
const TOGGLE_GLYPH: f32 = 14.0;
/// Collapsed strip: glyph rows hang from this far below the strip's top.
const STRIP_PAD_TOP: f32 = 10.0;
const STRIP_GLYPH_GAP: f32 = 14.0;
const BADGE_FONT: f32 = 10.0;
const BADGE_HEIGHT: f32 = 16.0;
const BADGE_PAD_X: f32 = 4.0;
/// Counts past this read as `99+`, so the badge never outgrows the strip.
const BADGE_MAX: usize = 99;

/// The dock's own tone. The property panel beside it is `theme.card`, and
/// the shell separates surfaces by tone, never by hairlines — so the dock
/// takes the next step up (`muted`) and the two columns read as two.
fn surface(theme: &Theme) -> Color {
    theme.muted
}

/// The dock for one frame.
pub struct ProjectsDock<'a> {
    /// The list the open dock paints under its header.
    pub list: ProjectsPanel<'a>,
    /// The header title (the dock's one visible name).
    pub title: &'a str,
    /// The cursor is over the collapse / expand chevron.
    pub toggle_hover: bool,
    /// The cursor is over the "Open project" button.
    pub open_project_hover: bool,
    /// Whether any project is listed; the empty strip carries an "Open
    /// project" glyph in place of the project count.
    pub has_projects: bool,
}

impl ProjectsDock<'_> {
    /// Paint the dock into its layout.
    pub fn paint(&self, cx: &mut PaintCx<'_>, layout: &ProjectsDockLayout, theme: &Theme) {
        if layout.open {
            self.paint_open(cx, layout, theme);
        } else {
            self.paint_strip(cx, layout, theme);
        }
    }

    fn paint_open(&self, cx: &mut PaintCx<'_>, layout: &ProjectsDockLayout, theme: &Theme) {
        cx.backend.fill_rect(layout.rect, surface(theme));
        let header = layout.header;
        draw_icon(
            cx.backend,
            Icon::FolderOpen,
            Point2D::new(
                header.origin.x + HEADER_PAD_LEFT,
                header.origin.y + (header.size.y - FOLDER_GLYPH) / 2.0,
            ),
            FOLDER_GLYPH,
            theme.muted_foreground,
            1.6,
        );
        let title_x = header.origin.x + HEADER_PAD_LEFT + FOLDER_GLYPH + TITLE_GAP;
        let title_right = layout
            .open_project
            .map_or(layout.toggle.origin.x, |button| button.origin.x);
        let title_max = (title_right - TITLE_GAP - title_x).max(0.0);
        let title = crate::util::ellipsize_to_width(self.title, title_max, |candidate| {
            text_metrics::measure_chrome_weighted(cx.backend, candidate, TITLE_FONT, TITLE_WEIGHT)
        });
        draw_label(
            cx,
            &title,
            TITLE_FONT,
            TITLE_WEIGHT,
            theme.foreground,
            Point2D::new(
                title_x,
                jian_widgets::centered_text_baseline_y(header, TITLE_FONT),
            ),
        );
        if let Some(button) = layout.open_project {
            self.paint_open_project(cx, button, theme);
        }
        // The chevron points at the edge the dock folds into.
        self.paint_toggle(cx, layout.toggle, Icon::ChevronRight, theme);
        if let Some(list) = &layout.list {
            self.list.paint_on(cx, list, theme, surface(theme));
        }
    }

    fn paint_strip(&self, cx: &mut PaintCx<'_>, layout: &ProjectsDockLayout, theme: &Theme) {
        let rect = layout.rect;
        cx.backend.fill_rect(rect, surface(theme));
        if self.toggle_hover {
            cx.backend.fill_rect(layout.toggle, theme.button_hover);
        }
        let centre_x = rect.origin.x + rect.size.x / 2.0;
        let mut y = rect.origin.y + STRIP_PAD_TOP;
        // The chevron points into the window: that is where the dock opens.
        draw_icon(
            cx.backend,
            Icon::ChevronLeft,
            Point2D::new(centre_x - TOGGLE_GLYPH / 2.0, y),
            TOGGLE_GLYPH,
            if self.toggle_hover {
                theme.foreground
            } else {
                theme.muted_foreground
            },
            1.6,
        );
        y += TOGGLE_GLYPH + STRIP_GLYPH_GAP;
        if let Some(button) = layout.open_project.filter(|_| !self.has_projects) {
            // An empty dock has nothing to count: the strip offers the one
            // thing there is to do.
            self.paint_open_project(cx, button, theme);
            return;
        }
        draw_icon(
            cx.backend,
            Icon::FolderOpen,
            Point2D::new(centre_x - FOLDER_GLYPH / 2.0, y),
            FOLDER_GLYPH,
            theme.muted_foreground,
            1.6,
        );
        y += FOLDER_GLYPH + 8.0;
        self.paint_count_badge(cx, centre_x, y, theme);
    }

    fn paint_count_badge(&self, cx: &mut PaintCx<'_>, centre_x: f32, top: f32, theme: &Theme) {
        let count = self.list.projects.len();
        let label = if count > BADGE_MAX {
            format!("{BADGE_MAX}+")
        } else {
            count.to_string()
        };
        let text_width = text_metrics::measure_chrome(cx.backend, &label, BADGE_FONT);
        let width = (text_width + BADGE_PAD_X * 2.0).max(BADGE_HEIGHT);
        let badge = Rect::xywh(centre_x - width / 2.0, top, width, BADGE_HEIGHT);
        cx.backend
            .fill_round_rect(badge, BADGE_HEIGHT / 2.0, theme.row_selected);
        draw_label(
            cx,
            &label,
            BADGE_FONT,
            400,
            theme.foreground,
            Point2D::new(
                badge.origin.x + (badge.size.x - text_width) / 2.0,
                jian_widgets::centered_text_baseline_y(badge, BADGE_FONT),
            ),
        );
    }

    fn paint_open_project(&self, cx: &mut PaintCx<'_>, button: Rect, theme: &Theme) {
        if self.open_project_hover {
            cx.backend.fill_rect(button, theme.button_hover);
        }
        draw_icon(
            cx.backend,
            Icon::FolderPlus,
            Point2D::new(
                button.origin.x + (button.size.x - TOGGLE_GLYPH) / 2.0,
                button.origin.y + (button.size.y - TOGGLE_GLYPH) / 2.0,
            ),
            TOGGLE_GLYPH,
            if self.open_project_hover {
                theme.foreground
            } else {
                theme.muted_foreground
            },
            1.6,
        );
    }

    fn paint_toggle(&self, cx: &mut PaintCx<'_>, toggle: Rect, icon: Icon, theme: &Theme) {
        if self.toggle_hover {
            cx.backend.fill_rect(toggle, theme.button_hover);
        }
        draw_icon(
            cx.backend,
            icon,
            Point2D::new(
                toggle.origin.x + (DOCK_TOGGLE_SIZE - TOGGLE_GLYPH) / 2.0,
                toggle.origin.y + (toggle.size.y - TOGGLE_GLYPH) / 2.0,
            ),
            TOGGLE_GLYPH,
            if self.toggle_hover {
                theme.foreground
            } else {
                theme.muted_foreground
            },
            1.6,
        );
    }
}

#[cfg(test)]
#[path = "projects_dock_tests.rs"]
mod projects_dock_tests;
