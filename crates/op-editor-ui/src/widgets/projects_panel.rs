//! The Projects dock's list: every project and the design files in it.
//!
//! Figma UI3 look — flat, borderless, tone-separated. A project header
//! carries a chevron, the project name in semibold, a muted folder hint and
//! an aggregate chip on the right (`n need review` / `n running`, else the
//! file count); hovering it swaps the chip for a `+` that starts a new
//! design in that project (and, on a project the user opened, a refresh and a
//! close button). File rows hang indented below, grouped under a muted folder
//! sub-header per directory (`projects_panel_rows.rs`), with the agent
//! status as a leading dot, an unsaved bullet after the label, the active
//! document filled with `row_selected_primary` and files that are not open
//! in the muted tone.
//!
//! The widget only paints. Where rows sit is [`ProjectsPanelLayout`], what a
//! press means is `projects_panel_flow`, and the data is the host's
//! `ProjectView` rows.

use op_editor_core::{
    DocumentTabStatus, ProjectFileKind, ProjectFileView, ProjectRenameState, ProjectRowTarget,
    ProjectScanState, ProjectView,
};

use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::projects_panel_layout::{
    ProjectRow, ProjectsPanelLayout, RowKind, PLUS_SIZE, ROW_PAD_RIGHT,
};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

#[path = "projects_panel_fig.rs"]
mod fig;
#[path = "projects_panel_rename.rs"]
mod rename;
#[path = "projects_panel_rows.rs"]
mod rows;

pub(crate) use rename::rename_field_rect;

const CHEVRON_SIZE: f32 = 14.0;
const CHEVRON_PAD_LEFT: f32 = 6.0;
const HEADER_FONT: f32 = 12.0;
pub(crate) const FILE_FONT: f32 = 12.0;
const HINT_FONT: f32 = 11.0;
const CHIP_FONT: f32 = 11.0;
const NAME_GAP: f32 = 6.0;
/// The hint (parent folder) is dropped when less than this is left for it.
const HINT_MIN_WIDTH: f32 = 48.0;
/// Where a file row's content starts, from its row's left edge: under the
/// header's name, past the chevron.
pub(crate) const FILE_INDENT: f32 = CHEVRON_PAD_LEFT + CHEVRON_SIZE + 6.0;
/// Extra indent of the files under a folder sub-header.
pub(crate) const FOLDER_FILE_INDENT: f32 = 14.0;
/// Width of the leading slot that holds a file's status dot, so labels line
/// up whether or not a file has one.
pub(crate) const STATUS_SLOT: f32 = 14.0;
const STATUS_DOT: f32 = 6.0;
const DIRTY_DOT: f32 = 5.0;
const DIRTY_GAP: f32 = 6.0;
pub(crate) const PLUS_GLYPH: f32 = 14.0;
pub(crate) const SEMIBOLD: u16 = 600;

/// The list's translated strings. `needs_review`, `running` and `truncated`
/// carry a `{{count}}` placeholder.
#[derive(Debug, Clone, Copy)]
pub struct ProjectsPanelText<'a> {
    /// The empty dock's headline.
    pub empty: &'a str,
    pub new_design: &'a str,
    pub needs_review: &'a str,
    pub running: &'a str,
    /// The empty dock's "Open project" button (also the header button's tip).
    pub open_project: &'a str,
    /// The empty dock's one-line hint under the headline.
    pub open_hint: &'a str,
    /// Tooltip of a project's close button.
    pub close: &'a str,
    /// Tooltip of a project's refresh button.
    pub refresh: &'a str,
    pub scanning: &'a str,
    pub slow: &'a str,
    pub unreadable: &'a str,
    pub truncated: &'a str,
    /// An opened folder with no `.op` file in it.
    pub no_designs: &'a str,
    /// A `.fig` row's status words.
    pub fig_convert: &'a str,
    pub fig_converted: &'a str,
    pub fig_converting: &'a str,
    pub fig_failed: &'a str,
    /// "Convert all ({{count}})" action row.
    pub fig_convert_all: &'a str,
    /// Tooltips: convert one, open the converted file, a failure (carries
    /// `{{reason}}`) and convert all.
    pub fig_convert_tip: &'a str,
    pub fig_converted_tip: &'a str,
    pub fig_failed_tip: &'a str,
    pub fig_convert_all_tip: &'a str,
    /// Tooltip of a file row's pencil button.
    pub rename: &'a str,
    /// Shown in the rename field while it is empty.
    pub rename_placeholder: &'a str,
    /// The line under the field: a name the rules refuse, or one already taken.
    pub rename_invalid: &'a str,
    pub rename_exists: &'a str,
}

/// The widget for one frame.
pub struct ProjectsPanel<'a> {
    pub projects: &'a [ProjectView],
    pub hover: Option<ProjectRowTarget>,
    /// The file name being edited, when a rename is open.
    pub rename: Option<&'a ProjectRenameState>,
    /// Frame time, for the caret blink.
    pub now_ms: u64,
    pub text: &'a ProjectsPanelText<'a>,
}

impl ProjectsPanel<'_> {
    /// Paint the list into the layout's viewport, on the card tone.
    pub fn paint(&self, cx: &mut PaintCx<'_>, layout: &ProjectsPanelLayout, theme: &Theme) {
        self.paint_on(cx, layout, theme, theme.card);
    }

    /// Paint the list on a surface of the caller's tone — the dock sets
    /// itself apart from the property panel beside it by tone alone.
    pub fn paint_on(
        &self,
        cx: &mut PaintCx<'_>,
        layout: &ProjectsPanelLayout,
        theme: &Theme,
        surface: Color,
    ) {
        cx.backend.fill_rect(layout.panel, surface);
        if layout.empty {
            self.paint_empty(cx, layout, theme);
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
                RowKind::Folder(project, file) => self.paint_folder(cx, theme, row, project, file),
                RowKind::Status(project) => self.paint_status(cx, theme, row, project),
                RowKind::NoDesigns(_) => self.paint_note(cx, theme, row, self.text.no_designs),
                RowKind::ConvertAll(project, folder) => {
                    self.paint_convert_all(cx, theme, row, project, folder)
                }
            }
        }
        self.paint_rename_error(cx, theme, layout, surface);
        cx.backend.restore();
        self.paint_button_tooltip(cx, theme, layout);
        self.paint_fig_tooltip(cx, theme, layout);
    }

    fn paint_header(&self, cx: &mut PaintCx<'_>, theme: &Theme, row: &ProjectRow, index: usize) {
        let Some(project) = self.projects.get(index) else {
            return;
        };
        let hovered = matches!(
            self.hover,
            Some(
                ProjectRowTarget::Header(i)
                    | ProjectRowTarget::Plus(i)
                    | ProjectRowTarget::Close(i)
                    | ProjectRowTarget::Refresh(i)
            ) if i == index
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

        // Trailing slot: the buttons while hovered, else the aggregate chip.
        let right_edge = rect.origin.x + rect.size.x - ROW_PAD_RIGHT;
        let mut trailing_left = right_edge;
        if hovered {
            let buttons = [
                (
                    row.refresh,
                    Icon::RotateCw,
                    ProjectRowTarget::Refresh(index),
                ),
                (row.close, Icon::Close, ProjectRowTarget::Close(index)),
                (row.plus, Icon::Plus, ProjectRowTarget::Plus(index)),
            ];
            for (button, icon, target) in buttons {
                let Some(button) = button else { continue };
                let hot = self.hover == Some(target);
                paint_header_button(cx, theme, button, icon, hot);
                trailing_left = trailing_left.min(button.origin.x);
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
        // While the first scan runs the count would read as "0 designs".
        if project.scan == ProjectScanState::Scanning {
            return (String::new(), theme.muted_foreground);
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
        if matches!(file.kind, ProjectFileKind::Figma(_)) {
            self.paint_fig_file(cx, theme, row, file, project, index);
            return;
        }
        if let Some(edit) = self.rename.filter(|edit| edit.path == file.path) {
            self.paint_rename_row(cx, theme, row, file, edit);
            return;
        }
        let rect = row.rect;
        let hovered = matches!(
            self.hover,
            Some(ProjectRowTarget::File(p, f) | ProjectRowTarget::RenameFile(p, f))
                if p == project && f == index
        );
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

        let slot_x = rect.origin.x
            + FILE_INDENT
            + if file.folder.is_empty() {
                0.0
            } else {
                FOLDER_FILE_INDENT
            };
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
        // The pencil replaces the right edge of a hovered row, so the label
        // stops short of it.
        let pencil = row.rename.filter(|_| hovered);
        let right_limit = pencil.map_or(rect.origin.x + rect.size.x - ROW_PAD_RIGHT, |button| {
            button.origin.x - NAME_GAP
        });
        let max = (right_limit - label_x - bullet_span).max(0.0);
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
        if let Some(button) = pencil {
            let hot = self.hover == Some(ProjectRowTarget::RenameFile(project, index));
            paint_header_button(cx, theme, button, Icon::Pencil, hot);
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

/// One of a header's icon buttons: a flat wash and a glyph that darkens when
/// the pointer is on it.
fn paint_header_button(cx: &mut PaintCx<'_>, theme: &Theme, button: Rect, icon: Icon, hot: bool) {
    if hot {
        cx.backend.fill_rect(button, theme.button_hover);
    }
    draw_icon(
        cx.backend,
        icon,
        Point2D::new(
            button.origin.x + (PLUS_SIZE - PLUS_GLYPH) / 2.0,
            button.origin.y + (button.size.y - PLUS_GLYPH) / 2.0,
        ),
        PLUS_GLYPH,
        if hot {
            theme.foreground
        } else {
            theme.muted_foreground
        },
        1.6,
    );
}

pub(crate) fn draw_label(
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
