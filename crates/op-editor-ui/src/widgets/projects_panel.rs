//! The left rail's Projects tab: every project and the design files in it.
//!
//! Figma UI3 look — flat, borderless, tone-separated. A project header
//! carries a chevron, the project name in semibold, a muted folder hint and
//! an aggregate chip on the right (`n need review` / `n running`, else the
//! file count); hovering it swaps the chip for a `+` that starts a new
//! design in that project. File rows hang indented below, with the agent
//! status as a leading dot, an unsaved bullet after the label, the active
//! document filled with `row_selected_primary` and files that are not open
//! in the muted tone.
//!
//! The widget only paints. Where rows sit is [`ProjectsPanelLayout`], what a
//! press means is `projects_panel_flow`, and the data is the host's
//! `ProjectView` rows.

use op_editor_core::{DocumentTabStatus, ProjectFileView, ProjectRowTarget, ProjectView};

use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_panel_layout::{
    ProjectRow, ProjectsPanelLayout, RowKind, ROW_PAD_RIGHT,
};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

const CHEVRON_SIZE: f32 = 14.0;
const CHEVRON_PAD_LEFT: f32 = 6.0;
const HEADER_FONT: f32 = 12.0;
const FILE_FONT: f32 = 12.0;
const HINT_FONT: f32 = 11.0;
const CHIP_FONT: f32 = 11.0;
const NAME_GAP: f32 = 6.0;
/// The hint (parent folder) is dropped when less than this is left for it.
const HINT_MIN_WIDTH: f32 = 48.0;
/// Where a file row's content starts, from its row's left edge: under the
/// header's name, past the chevron.
const FILE_INDENT: f32 = CHEVRON_PAD_LEFT + CHEVRON_SIZE + 6.0;
/// Width of the leading slot that holds a file's status dot, so labels line
/// up whether or not a file has one.
const STATUS_SLOT: f32 = 14.0;
const STATUS_DOT: f32 = 6.0;
const DIRTY_DOT: f32 = 5.0;
const DIRTY_GAP: f32 = 6.0;
const PLUS_GLYPH: f32 = 14.0;
const EMPTY_FONT: f32 = 12.0;
const EMPTY_PAD_TOP: f32 = 28.0;
const SEMIBOLD: u16 = 600;

/// The tab's translated strings. `needs_review` and `running` carry a
/// `{{count}}` placeholder.
#[derive(Debug, Clone, Copy)]
pub struct ProjectsPanelText<'a> {
    pub empty: &'a str,
    pub new_design: &'a str,
    pub needs_review: &'a str,
    pub running: &'a str,
}

/// The widget for one frame.
pub struct ProjectsPanel<'a> {
    pub projects: &'a [ProjectView],
    pub hover: Option<ProjectRowTarget>,
    pub text: &'a ProjectsPanelText<'a>,
}

impl ProjectsPanel<'_> {
    /// Paint the tab body into the layout's viewport.
    pub fn paint(&self, cx: &mut PaintCx<'_>, layout: &ProjectsPanelLayout, theme: &Theme) {
        cx.backend.fill_rect(layout.panel, theme.card);
        if layout.empty {
            self.paint_empty(cx, layout.panel, theme);
            return;
        }
        cx.backend.save();
        cx.backend.clip_rect(layout.panel);
        for row in &layout.rows {
            // Rows scrolled out of the viewport cost nothing.
            if row.rect.origin.y + row.rect.size.y < layout.panel.origin.y
                || row.rect.origin.y > layout.panel.origin.y + layout.panel.size.y
            {
                continue;
            }
            match row.kind {
                RowKind::Header(project) => self.paint_header(cx, theme, row, project),
                RowKind::File(project, file) => {
                    if let Some(view) = self.projects.get(project).and_then(|p| p.files.get(file)) {
                        self.paint_file(cx, theme, row, view, project, file);
                    }
                }
                RowKind::NewDesign(project) => self.paint_new_design(cx, theme, row, project),
            }
        }
        cx.backend.restore();
    }

    fn paint_empty(&self, cx: &mut PaintCx<'_>, panel: Rect, theme: &Theme) {
        let max = (panel.size.x - 32.0).max(0.0);
        let text = text_metrics::fit_chrome(cx.backend, self.text.empty, max, EMPTY_FONT);
        let width = text_metrics::measure_chrome(cx.backend, &text, EMPTY_FONT);
        draw_label(
            cx,
            &text,
            EMPTY_FONT,
            400,
            theme.muted_foreground,
            Point2D::new(
                panel.origin.x + (panel.size.x - width) / 2.0,
                panel.origin.y + EMPTY_PAD_TOP,
            ),
        );
    }

    fn paint_header(&self, cx: &mut PaintCx<'_>, theme: &Theme, row: &ProjectRow, index: usize) {
        let Some(project) = self.projects.get(index) else {
            return;
        };
        let hovered = matches!(
            self.hover,
            Some(ProjectRowTarget::Header(i) | ProjectRowTarget::Plus(i)) if i == index
        );
        if hovered {
            cx.backend.fill_rect(row.rect, theme.accent);
        }
        let rect = row.rect;
        let baseline = jian_widgets::centered_text_baseline_y(rect, HEADER_FONT);
        let chevron = if project.expanded {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        draw_icon(
            cx.backend,
            chevron,
            Point2D::new(
                rect.origin.x + CHEVRON_PAD_LEFT,
                rect.origin.y + (rect.size.y - CHEVRON_SIZE) / 2.0,
            ),
            CHEVRON_SIZE,
            theme.muted_foreground,
            1.6,
        );

        // Trailing slot: the `+` while hovered, else the aggregate chip.
        let right_edge = rect.origin.x + rect.size.x - ROW_PAD_RIGHT;
        let mut trailing_left = right_edge;
        if hovered {
            if let Some(plus) = row.plus {
                let plus_hot = self.hover == Some(ProjectRowTarget::Plus(index));
                if plus_hot {
                    cx.backend.fill_rect(plus, theme.button_hover);
                }
                draw_icon(
                    cx.backend,
                    Icon::Plus,
                    Point2D::new(
                        plus.origin.x + (plus.size.x - PLUS_GLYPH) / 2.0,
                        plus.origin.y + (plus.size.y - PLUS_GLYPH) / 2.0,
                    ),
                    PLUS_GLYPH,
                    if plus_hot {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    },
                    1.6,
                );
                trailing_left = plus.origin.x;
            }
        } else {
            let (label, color) = self.chip(theme, project);
            let width = text_metrics::measure_chrome(cx.backend, &label, CHIP_FONT);
            trailing_left = right_edge - width;
            draw_label(
                cx,
                &label,
                CHIP_FONT,
                400,
                color,
                Point2D::new(
                    trailing_left,
                    jian_widgets::centered_text_baseline_y(rect, CHIP_FONT),
                ),
            );
        }

        // Name, then the muted folder hint when there is room for it.
        let name_x = rect.origin.x + CHEVRON_PAD_LEFT + CHEVRON_SIZE + 4.0;
        let name_max = (trailing_left - NAME_GAP - name_x).max(0.0);
        let name = fit_weighted(cx, &project.name, name_max, HEADER_FONT, SEMIBOLD);
        let name_width =
            text_metrics::measure_chrome_weighted(cx.backend, &name, HEADER_FONT, SEMIBOLD);
        draw_label(
            cx,
            &name,
            HEADER_FONT,
            SEMIBOLD,
            theme.foreground,
            Point2D::new(name_x, baseline),
        );
        // A collapsed project hides its files, so it carries their unsaved
        // state itself: the same bullet a file row shows.
        let mut after_name = name_x + name_width;
        if !project.expanded
            && project.any_dirty()
            && after_name + DIRTY_GAP + DIRTY_DOT <= trailing_left
        {
            cx.backend.fill_round_rect(
                Rect::xywh(
                    after_name + DIRTY_GAP,
                    rect.origin.y + (rect.size.y - DIRTY_DOT) / 2.0,
                    DIRTY_DOT,
                    DIRTY_DOT,
                ),
                DIRTY_DOT / 2.0,
                theme.foreground,
            );
            after_name += DIRTY_GAP + DIRTY_DOT;
        }
        let hint_x = after_name + NAME_GAP;
        let hint_max = trailing_left - NAME_GAP - hint_x;
        if let Some(tail) = folder_hint(&project.root).filter(|_| hint_max >= HINT_MIN_WIDTH) {
            let tail = text_metrics::fit_chrome(cx.backend, &tail, hint_max, HINT_FONT);
            draw_label(
                cx,
                &tail,
                HINT_FONT,
                400,
                theme.muted_foreground,
                Point2D::new(
                    hint_x,
                    jian_widgets::centered_text_baseline_y(rect, HINT_FONT),
                ),
            );
        }
    }

    /// The header's aggregate chip: what needs the owner first, else what
    /// is running, else the plain file count.
    fn chip(&self, theme: &Theme, project: &ProjectView) -> (String, Color) {
        let review = project.needs_review_count();
        if review > 0 {
            return (
                self.text
                    .needs_review
                    .replace("{{count}}", &review.to_string()),
                theme.status_warning,
            );
        }
        let running = project.running_count();
        if running > 0 {
            return (
                self.text.running.replace("{{count}}", &running.to_string()),
                theme.primary,
            );
        }
        (project.files.len().to_string(), theme.muted_foreground)
    }

    fn paint_file(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        file: &ProjectFileView,
        project: usize,
        index: usize,
    ) {
        let rect = row.rect;
        let hovered = self.hover == Some(ProjectRowTarget::File(project, index));
        if file.active {
            cx.backend.fill_rect(rect, theme.row_selected_primary);
        } else if hovered {
            cx.backend.fill_rect(rect, theme.accent);
        }
        let text_color = if file.active || hovered || file.open_id.is_some() {
            theme.foreground
        } else {
            theme.muted_foreground
        };

        let slot_x = rect.origin.x + FILE_INDENT;
        if let Some(color) = status_color(theme, file.status) {
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
        let label_x = slot_x + STATUS_SLOT;
        let bullet_span = if file.dirty {
            DIRTY_GAP + DIRTY_DOT
        } else {
            0.0
        };
        let max = (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - label_x - bullet_span).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, &file.label, max, FILE_FONT);
        let width = text_metrics::measure_chrome(cx.backend, &label, FILE_FONT);
        draw_label(
            cx,
            &label,
            FILE_FONT,
            400,
            text_color,
            Point2D::new(
                label_x,
                jian_widgets::centered_text_baseline_y(rect, FILE_FONT),
            ),
        );
        if file.dirty {
            cx.backend.fill_round_rect(
                Rect::xywh(
                    label_x + width + DIRTY_GAP,
                    rect.origin.y + (rect.size.y - DIRTY_DOT) / 2.0,
                    DIRTY_DOT,
                    DIRTY_DOT,
                ),
                DIRTY_DOT / 2.0,
                text_color,
            );
        }
    }

    /// The row an expanded project with no files shows, so the project
    /// still offers a way to start.
    fn paint_new_design(
        &self,
        cx: &mut PaintCx<'_>,
        theme: &Theme,
        row: &ProjectRow,
        index: usize,
    ) {
        let rect = row.rect;
        let hovered = self.hover == Some(ProjectRowTarget::NewDesignRow(index));
        if hovered {
            cx.backend.fill_rect(rect, theme.accent);
        }
        let color = if hovered {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        let glyph_x = rect.origin.x + FILE_INDENT;
        draw_icon(
            cx.backend,
            Icon::Plus,
            Point2D::new(glyph_x, rect.origin.y + (rect.size.y - PLUS_GLYPH) / 2.0),
            PLUS_GLYPH,
            color,
            1.6,
        );
        let label_x = glyph_x + PLUS_GLYPH + 6.0;
        let max = (rect.origin.x + rect.size.x - ROW_PAD_RIGHT - label_x).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, self.text.new_design, max, FILE_FONT);
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
    }
}

fn draw_label(
    cx: &mut PaintCx<'_>,
    text: &str,
    font: f32,
    weight: u16,
    color: Color,
    origin: Point2D,
) {
    if text.is_empty() {
        return;
    }
    cx.backend.draw_text(
        &TextLayout::single_run(text, "system-ui", font, color.to_jian(), Point2D::ZERO)
            .with_font_weight(weight),
        origin,
    );
}

/// Ellipsize `text` measured at `weight`, the weight it is drawn in.
fn fit_weighted(cx: &mut PaintCx<'_>, text: &str, max_w: f32, font: f32, weight: u16) -> String {
    crate::util::ellipsize_to_width(text, max_w, |candidate| {
        text_metrics::measure_chrome_weighted(cx.backend, candidate, font, weight)
    })
}

/// The folder that holds the project, shown muted after its name; `None`
/// when the root has no parent segment worth showing.
pub fn folder_hint(root: &str) -> Option<String> {
    let mut segments = root
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty());
    let _name = segments.next_back()?;
    segments.next_back().map(str::to_string)
}

/// The dot colour of an agent status; `None` when idle.
fn status_color(theme: &Theme, status: DocumentTabStatus) -> Option<Color> {
    match status {
        DocumentTabStatus::Idle => None,
        DocumentTabStatus::Running => Some(theme.primary),
        DocumentTabStatus::NeedsReview => Some(theme.status_warning),
        DocumentTabStatus::Done => Some(theme.status_success),
        DocumentTabStatus::Error => Some(theme.destructive),
    }
}

#[cfg(test)]
#[path = "projects_panel_tests.rs"]
mod projects_panel_tests;
