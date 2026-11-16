//! Open-document tab strip in the `TopBar`'s middle title slot.
//!
//! With two or more documents open the strip replaces the centred file
//! name; with 0 or 1 the title UI is untouched. The strip is flat and
//! borderless (Figma UI3): the active tab is a filled `row_selected` block,
//! a hovered tab is a `accent` wash, and projects are told apart by a small
//! muted name before each group rather than by separator lines.
//!
//! Layout is a pure function of the tab rows, the slot and one text metric,
//! so paint, hit-test and the hover tooltip anchor all read the same rects.
//! When the tabs no longer fit at their minimum width the strip shows a
//! window of them that always contains the active tab, plus a trailing
//! `+N` chip standing for the hidden ones.

use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::text_metrics;
use crate::widgets::top_bar::*;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect, TextLayout};
use op_editor_core::{DocumentTabStatus, DocumentTabView, TopBarButton};

const TAB_HEIGHT: f32 = 28.0;
const TAB_MIN_WIDTH: f32 = 112.0;
const TAB_MAX_WIDTH: f32 = 176.0;
const TAB_GAP: f32 = 2.0;
/// Space between two project groups (stands in for a separator line).
const GROUP_GAP: f32 = 14.0;
const TAB_PAD_X: f32 = 10.0;
const TAB_FONT: f32 = 12.0;
const PROJECT_FONT: f32 = 11.0;
const PROJECT_LABEL_MAX_WIDTH: f32 = 110.0;
const PROJECT_LABEL_TRAIL: f32 = 6.0;
const STATUS_DOT: f32 = 6.0;
const STATUS_DOT_GAP: f32 = 6.0;
const CLOSE_SIZE: f32 = 16.0;
const CLOSE_PAD_RIGHT: f32 = 6.0;
const CLOSE_GLYPH: f32 = 10.0;
/// Narrowest tab that still shows a close button beside a short label.
const CLOSE_MIN_TAB_WIDTH: f32 = TAB_PAD_X + 24.0 + CLOSE_SIZE + CLOSE_PAD_RIGHT;
const MORE_WIDTH: f32 = 36.0;

/// One visible tab: where it sits and where its close (x) sits.
#[derive(Debug, Clone)]
pub(super) struct DocumentTabCell {
    /// Index into `EditorUiState.document_tabs`.
    pub index: usize,
    pub rect: Rect,
    pub close_rect: Rect,
}

/// A project name painted before the first tab of its group.
#[derive(Debug, Clone)]
pub(super) struct ProjectLabel {
    pub text: String,
    pub rect: Rect,
}

/// The `+N` chip that stands for tabs outside the visible window.
#[derive(Debug, Clone)]
pub(super) struct DocumentTabsMore {
    pub rect: Rect,
    /// The tab a press on the chip switches to: the first hidden one to the
    /// right, or the last hidden one to the left when none remain there.
    pub target: usize,
    pub hidden: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DocumentTabsLayout {
    pub cells: Vec<DocumentTabCell>,
    pub labels: Vec<ProjectLabel>,
    pub more: Option<DocumentTabsMore>,
}

impl DocumentTabsLayout {
    /// The hit under `point`, close button first.
    pub(super) fn hit(&self, point: Point2D) -> Option<TopBarHit> {
        for cell in &self.cells {
            if cell.close_rect.size.x > 0.0 && cell.close_rect.contains(point) {
                return Some(TopBarHit::CloseDocumentTab(cell.index));
            }
            if cell.rect.contains(point) {
                return Some(TopBarHit::DocumentTab(cell.index));
            }
        }
        let more = self.more.as_ref()?;
        more.rect
            .contains(point)
            .then_some(TopBarHit::DocumentTab(more.target))
    }

    pub(super) fn cell(&self, index: usize) -> Option<&DocumentTabCell> {
        self.cells.iter().find(|cell| cell.index == index)
    }
}

/// Per-tab facts the window search needs, computed once.
struct TabMetrics {
    /// Width of the project label shown at this tab when it opens a group
    /// (or the window), `None` when no label applies.
    label_width: Vec<Option<f32>>,
    starts_group: Vec<bool>,
}

impl TabMetrics {
    fn new(tabs: &[DocumentTabView], mut measure: impl FnMut(&str, f32) -> f32) -> Self {
        let mut distinct: Vec<&str> = Vec::new();
        for tab in tabs {
            if !distinct.contains(&tab.project.as_str()) {
                distinct.push(&tab.project);
            }
        }
        let show_projects = distinct.len() >= 2;
        let label_width = tabs
            .iter()
            .map(|tab| {
                (show_projects && !tab.project.is_empty()).then(|| {
                    measure(&tab.project, PROJECT_FONT).min(PROJECT_LABEL_MAX_WIDTH)
                        + PROJECT_LABEL_TRAIL
                })
            })
            .collect();
        let starts_group = tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| i == 0 || tabs[i - 1].project != tab.project)
            .collect();
        Self {
            label_width,
            starts_group,
        }
    }

    /// Width taken by everything except the tabs themselves for the window
    /// `start..end`: gaps between tabs and the project labels.
    fn overhead(&self, start: usize, end: usize) -> f32 {
        let mut total = 0.0;
        for i in start..end {
            if i > start {
                total += if self.starts_group[i] {
                    GROUP_GAP
                } else {
                    TAB_GAP
                };
            }
            if i == start || self.starts_group[i] {
                total += self.label_width[i].unwrap_or(0.0);
            }
        }
        total
    }
}

/// Lay the tab rows out inside `slot`.
pub(super) fn layout_document_tabs(
    tabs: &[DocumentTabView],
    slot: Rect,
    measure: impl FnMut(&str, f32) -> f32,
) -> DocumentTabsLayout {
    let count = tabs.len();
    if count == 0 || slot.size.x <= 0.0 {
        return DocumentTabsLayout::default();
    }
    let metrics = TabMetrics::new(tabs, measure);
    let more_span = TAB_GAP + MORE_WIDTH;
    let min_total = |start: usize, end: usize, with_more: bool| {
        metrics.overhead(start, end)
            + (end - start) as f32 * TAB_MIN_WIDTH
            + if with_more { more_span } else { 0.0 }
    };

    let (start, end) = if min_total(0, count, false) <= slot.size.x {
        (0, count)
    } else {
        let active = tabs.iter().position(|tab| tab.active).unwrap_or(0);
        // The leftmost window that still contains the active tab, so the
        // strip scrolls as little as possible when the user switches.
        (0..=active)
            .find_map(|start| {
                let end = (start + 1..=count)
                    .take_while(|&end| min_total(start, end, true) <= slot.size.x)
                    .last()?;
                (end > active).then_some((start, end))
            })
            .unwrap_or((active, active + 1))
    };

    let hidden = count - (end - start);
    let tab_span = end - start;
    let more_fits = hidden > 0 && min_total(start, end, true) <= slot.size.x;
    let reserved = if more_fits { more_span } else { 0.0 };
    let available = slot.size.x - metrics.overhead(start, end) - reserved;
    let tab_width = (available / tab_span as f32).clamp(0.0, TAB_MAX_WIDTH);

    let tab_y = slot.origin.y + (slot.size.y - TAB_HEIGHT) / 2.0;
    let mut layout = DocumentTabsLayout::default();
    let mut x = slot.origin.x;
    for index in start..end {
        if index > start {
            x += if metrics.starts_group[index] {
                GROUP_GAP
            } else {
                TAB_GAP
            };
        }
        if index == start || metrics.starts_group[index] {
            if let Some(label_width) = metrics.label_width[index] {
                layout.labels.push(ProjectLabel {
                    text: tabs[index].project.clone(),
                    rect: Rect::xywh(x, tab_y, label_width, TAB_HEIGHT),
                });
                x += label_width;
            }
        }
        let rect = Rect::xywh(x, tab_y, tab_width, TAB_HEIGHT);
        // A tab squeezed below its minimum (only when even one tab cannot get
        // `TAB_MIN_WIDTH`) has no room for the close button beside its label;
        // it gets an empty rect at the tab's right edge instead of one that
        // would spill over the project label next to it.
        let close_rect = if tab_width >= CLOSE_MIN_TAB_WIDTH {
            Rect::xywh(
                rect.origin.x + rect.size.x - CLOSE_PAD_RIGHT - CLOSE_SIZE,
                tab_y + (TAB_HEIGHT - CLOSE_SIZE) / 2.0,
                CLOSE_SIZE,
                CLOSE_SIZE,
            )
        } else {
            Rect::xywh(rect.origin.x + rect.size.x, tab_y, 0.0, 0.0)
        };
        layout.cells.push(DocumentTabCell {
            index,
            rect,
            close_rect,
        });
        x += tab_width;
    }
    if more_fits {
        layout.more = Some(DocumentTabsMore {
            rect: Rect::xywh(x + TAB_GAP, tab_y, MORE_WIDTH, TAB_HEIGHT),
            target: if end < count { end } else { start - 1 },
            hidden,
        });
    }
    layout
}

impl TopBar {
    /// Whether the tab strip replaces the centred title. The rows only reach
    /// the bar when the host supports tabs, the chrome is not touch and the
    /// file chrome is shown, so a single count decides.
    pub(super) fn document_tabs_visible(&self) -> bool {
        self.document_tabs.len() > 1
    }

    /// Title layout while the strip is shown: the strip fills the slot, with
    /// the Git toggle kept as an icon-only button at its right end so the
    /// panel stays reachable.
    pub(super) fn tabs_title_layout(
        &self,
        slot: Rect,
        measure: impl FnMut(&str, f32) -> f32,
    ) -> super::top_bar_title::TopBarTitleLayout {
        let show_git = GIT_BUTTON_AVAILABLE
            && slot.size.x >= TAB_MIN_WIDTH + super::top_bar_title::GIT_GAP + ICON_BUTTON;
        let git_span = if show_git {
            super::top_bar_title::GIT_GAP + ICON_BUTTON
        } else {
            0.0
        };
        let strip_slot = Rect::xywh(
            slot.origin.x,
            slot.origin.y,
            (slot.size.x - git_span).max(0.0),
            slot.size.y,
        );
        let git_rect = show_git.then(|| {
            Rect::xywh(
                slot.origin.x + slot.size.x - ICON_BUTTON,
                slot.origin.y + (slot.size.y - ICON_BUTTON) / 2.0,
                ICON_BUTTON,
                ICON_BUTTON,
            )
        });
        super::top_bar_title::TopBarTitleLayout {
            file_name: String::new(),
            file_x: slot.origin.x,
            edited_x: None,
            git_rect,
            slot,
            tabs: Some(layout_document_tabs(
                &self.document_tabs,
                strip_slot,
                measure,
            )),
        }
    }

    /// Index of the tab whose body contains `point` (the `+N` chip is not a
    /// tab), measured the way paint measures.
    pub fn document_tab_index_at(
        &self,
        top_bar_rect: Rect,
        point: Point2D,
        measure: impl FnMut(&str, f32) -> f32,
    ) -> Option<usize> {
        if !self.document_tabs_visible() || !top_bar_rect.contains(point) {
            return None;
        }
        self.title_layout(top_bar_rect, measure)
            .tabs?
            .cells
            .iter()
            .find(|cell| cell.rect.contains(point))
            .map(|cell| cell.index)
    }

    fn document_tab_hovered(&self, index: usize) -> bool {
        matches!(
            self.hover,
            Some(TopBarButton::DocumentTab(i) | TopBarButton::CloseDocumentTab(i)) if i == index
        )
    }

    pub(super) fn paint_document_tabs(
        &self,
        cx: &mut PaintCx<'_>,
        layout: &DocumentTabsLayout,
        center_y: f32,
    ) {
        let theme = &self.theme;
        for label in &layout.labels {
            let text = text_metrics::fit_chrome(
                cx.backend,
                &label.text,
                (label.rect.size.x - PROJECT_LABEL_TRAIL).max(0.0),
                PROJECT_FONT,
            );
            cx.backend.draw_text(
                &TextLayout::single_run(
                    &text,
                    "system-ui",
                    PROJECT_FONT,
                    theme.muted_foreground.to_jian(),
                    Point2D::ZERO,
                ),
                Point2D::new(label.rect.origin.x, center_y + 4.0),
            );
        }
        for cell in &layout.cells {
            self.paint_document_tab(cx, cell, center_y);
        }
        if let Some(more) = &layout.more {
            let hovered = self.hover == Some(TopBarButton::DocumentTab(more.target));
            if hovered {
                cx.backend.fill_rect(more.rect, theme.accent);
            }
            let text = format!("+{}", more.hidden);
            let width = text_metrics::measure_chrome(cx.backend, &text, PROJECT_FONT);
            cx.backend.draw_text(
                &TextLayout::single_run(
                    &text,
                    "system-ui",
                    PROJECT_FONT,
                    theme.muted_foreground.to_jian(),
                    Point2D::ZERO,
                ),
                Point2D::new(
                    more.rect.origin.x + (more.rect.size.x - width) / 2.0,
                    center_y + 4.0,
                ),
            );
        }
    }

    fn paint_document_tab(&self, cx: &mut PaintCx<'_>, cell: &DocumentTabCell, center_y: f32) {
        let theme = &self.theme;
        let tab = &self.document_tabs[cell.index];
        let hovered = self.document_tab_hovered(cell.index);
        if tab.active {
            cx.backend.fill_rect(cell.rect, theme.row_selected);
        } else if hovered {
            cx.backend.fill_rect(cell.rect, theme.accent);
        }
        let text_color = if tab.active {
            theme.foreground
        } else if hovered {
            theme.accent_foreground
        } else {
            theme.muted_foreground
        };

        let mut x = cell.rect.origin.x + TAB_PAD_X;
        if let Some(color) = status_color(theme, tab.status) {
            cx.backend.fill_round_rect(
                Rect::xywh(x, center_y - STATUS_DOT / 2.0, STATUS_DOT, STATUS_DOT),
                STATUS_DOT / 2.0,
                color,
            );
            x += STATUS_DOT + STATUS_DOT_GAP;
        }
        // The unsaved bullet follows the label directly: on a flat, borderless
        // tab a bullet parked at the far edge reads as belonging to the
        // neighbouring tab.
        let bullet_span = if tab.dirty {
            STATUS_DOT_GAP + STATUS_DOT
        } else {
            0.0
        };
        let label_max = (cell.close_rect.origin.x - 4.0 - x - bullet_span).max(0.0);
        let label = text_metrics::fit_chrome(cx.backend, &tab.label, label_max, TAB_FONT);
        let label_width = text_metrics::measure_chrome(cx.backend, &label, TAB_FONT);
        cx.backend.draw_text(
            &TextLayout::single_run(
                &label,
                "system-ui",
                TAB_FONT,
                text_color.to_jian(),
                Point2D::ZERO,
            ),
            Point2D::new(x, center_y + 4.5),
        );
        if tab.dirty {
            cx.backend.fill_round_rect(
                Rect::xywh(
                    x + label_width + STATUS_DOT_GAP,
                    center_y - STATUS_DOT / 2.0,
                    STATUS_DOT,
                    STATUS_DOT,
                ),
                STATUS_DOT / 2.0,
                text_color,
            );
        }

        // Trailing slot: the close (x) while hovered, and always on the
        // active tab.
        let close_hovered = self.hover == Some(TopBarButton::CloseDocumentTab(cell.index));
        if (hovered || tab.active) && cell.close_rect.size.x > 0.0 {
            if close_hovered {
                cx.backend.fill_rect(cell.close_rect, theme.button_hover);
            }
            draw_icon(
                cx.backend,
                Icon::Close,
                Point2D::new(
                    cell.close_rect.origin.x + (CLOSE_SIZE - CLOSE_GLYPH) / 2.0,
                    cell.close_rect.origin.y + (CLOSE_SIZE - CLOSE_GLYPH) / 2.0,
                ),
                CLOSE_GLYPH,
                if close_hovered {
                    theme.foreground
                } else {
                    text_color
                },
                1.6,
            );
        }
    }
}

/// The dot colour of an agent status; `None` for an idle document.
fn status_color(theme: &Theme, status: DocumentTabStatus) -> Option<crate::Color> {
    match status {
        DocumentTabStatus::Idle => None,
        DocumentTabStatus::Running => Some(theme.primary),
        DocumentTabStatus::NeedsReview => Some(theme.status_warning),
        DocumentTabStatus::Done => Some(theme.status_success),
        DocumentTabStatus::Error => Some(theme.destructive),
    }
}

#[cfg(test)]
#[path = "top_bar_document_tabs_tests.rs"]
mod tests;
