//! Layout, hit-test and paint of the document tab strip.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use crate::widgets::{TopBarHit, TOP_BAR_HEIGHT};
use op_editor_core::editor_ui_state::EditorUiState;
use op_editor_core::{DocumentId, DocumentTabStatus};

fn width_of(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.6
}

fn tab(n: u64, project: &str) -> DocumentTabView {
    DocumentTabView {
        id: DocumentId(n),
        label: format!("doc-{n}.op"),
        project: project.to_string(),
        dirty: false,
        status: DocumentTabStatus::Idle,
        active: false,
    }
}

fn tabs(count: u64, active: u64) -> Vec<DocumentTabView> {
    (0..count)
        .map(|n| DocumentTabView {
            active: n == active,
            ..tab(n, "")
        })
        .collect()
}

fn bar(rows: Vec<DocumentTabView>) -> TopBar {
    let mut bar = TopBar::new("Untitled");
    bar.document_tabs = rows;
    bar
}

fn bar_rect(width: f32) -> Rect {
    Rect::xywh(0.0, 0.0, width, TOP_BAR_HEIGHT)
}

fn slot(width: f32) -> Rect {
    Rect::xywh(0.0, 6.0, width, 28.0)
}

fn centre(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn paint(bar: &TopBar, width: f32) -> CaptureBackend {
    let mut backend = CaptureBackend::default();
    let rect = bar_rect(width);
    let layout = bar
        .title_layout(rect, width_of)
        .tabs
        .expect("the strip is shown");
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    bar.paint_document_tabs(&mut cx, &layout, TOP_BAR_HEIGHT / 2.0);
    backend
}

/// Strokes painted by `glyphs` close icons.
fn close_strokes(glyphs: usize) -> usize {
    glyphs * Icon::Close.paths().len()
}

fn supported_ui(rows: Vec<DocumentTabView>) -> EditorUiState {
    EditorUiState {
        document_tabs_supported: true,
        document_tabs: rows,
        ..EditorUiState::default()
    }
}

#[test]
fn strip_hidden_with_single_document() {
    let ui = supported_ui(tabs(1, 0));
    let single = TopBar::for_editor_ui(&ui);
    assert!(!single.document_tabs_visible());

    let none = TopBar::for_editor_ui(&supported_ui(Vec::new()));
    let rect = bar_rect(1200.0);
    let with_one = single.title_layout(rect, width_of);
    let without = none.title_layout(rect, width_of);
    assert!(with_one.tabs.is_none());
    assert_eq!(with_one.file_x, without.file_x, "title is not moved");
    assert_eq!(with_one.file_name, without.file_name);
    assert_eq!(with_one.git_rect, without.git_rect);
}

#[test]
fn strip_hidden_when_unsupported() {
    let mut ui = supported_ui(tabs(3, 0));
    ui.document_tabs_supported = false;
    assert!(!TopBar::for_editor_ui(&ui).document_tabs_visible());

    // Touch chrome keeps its own navigation; the strip is deferred there.
    let mut touch = supported_ui(tabs(3, 0));
    touch.touch = true;
    assert!(!TopBar::for_editor_ui(&touch).document_tabs_visible());

    let shown = TopBar::for_editor_ui(&supported_ui(tabs(3, 0)));
    assert!(shown.document_tabs_visible());
    assert!(shown
        .title_layout(bar_rect(1200.0), width_of)
        .tabs
        .is_some());
}

#[test]
fn strip_takes_over_the_title_slot_and_keeps_git_reachable() {
    let bar = bar(tabs(3, 1));
    let rect = bar_rect(1200.0);
    let layout = bar.title_layout(rect, width_of);
    assert!(layout.file_name.is_empty());
    assert!(layout.edited_x.is_none());
    let tabs = layout.tabs.expect("strip");
    let last = tabs.cells.last().unwrap().rect;
    if let Some(git) = layout.git_rect {
        assert_eq!(git.size.x, ICON_BUTTON, "icon-only Git button");
        assert!(git.origin.x >= last.origin.x + last.size.x);
        assert!(git.origin.x + git.size.x <= layout.slot.origin.x + layout.slot.size.x + 0.01);
        assert_eq!(
            bar.hit_test_with_measure(rect, centre(git), width_of),
            Some(TopBarHit::ToggleGitPanel)
        );
    } else {
        assert!(!GIT_BUTTON_AVAILABLE);
    }
}

#[test]
fn hit_test_resolves_tab_close_and_gaps() {
    let bar = bar(tabs(3, 0));
    let rect = bar_rect(1200.0);
    let layout = bar.title_layout(rect, width_of).tabs.unwrap();
    assert_eq!(layout.cells.len(), 3);

    let second = &layout.cells[1];
    let body = Point2D::new(second.rect.origin.x + 14.0, second.rect.origin.y + 14.0);
    assert_eq!(
        bar.hit_test_with_measure(rect, body, width_of),
        Some(TopBarHit::DocumentTab(1))
    );
    assert_eq!(
        bar.hit_test_with_measure(rect, centre(second.close_rect), width_of),
        Some(TopBarHit::CloseDocumentTab(1))
    );
    // The estimating variant (web) walks the same layout.
    assert!(matches!(
        bar.hit_test(rect, centre(second.rect)),
        Some(TopBarHit::DocumentTab(_) | TopBarHit::CloseDocumentTab(_))
    ));
    // The gap between two tabs is a blank press.
    let first = &layout.cells[0];
    let gap = Point2D::new(
        first.rect.origin.x + first.rect.size.x + TAB_GAP / 2.0,
        first.rect.origin.y + 14.0,
    );
    assert_eq!(bar.hit_test_with_measure(rect, gap, width_of), None);
}

#[test]
fn dirty_tab_shows_unsaved_marker() {
    let mut rows = tabs(3, 0);
    rows[2].dirty = true;
    let bar = bar(rows);
    let layout = bar.title_layout(bar_rect(1200.0), width_of).tabs.unwrap();
    let dirty_tab = layout.cells[2].rect;
    let backend = paint(&bar, 1200.0);
    let bullet = backend
        .round_fills
        .iter()
        .find(|(rect, radius, color)| {
            *radius == STATUS_DOT / 2.0
                && rect.size.x == STATUS_DOT
                && dirty_tab.contains(centre(*rect))
                && *color == bar.theme.muted_foreground
        })
        .expect("the dirty tab paints an unsaved bullet");
    assert_eq!(bullet.0.size.y, STATUS_DOT);

    // Clean tabs paint no bullet; the active clean tab shows its close icon.
    let clean = paint(&self::bar(tabs(3, 0)), 1200.0);
    assert!(clean.round_fills.is_empty());
    assert_eq!(
        clean.svg_strokes.len(),
        close_strokes(1),
        "active tab shows its x"
    );
}

#[test]
fn hover_reveals_close_and_dirty_tab_keeps_its_bullet() {
    let mut rows = tabs(3, 0);
    rows[1].dirty = true;
    let mut bar = bar(rows);
    bar.hover = Some(TopBarButton::DocumentTab(1));
    let backend = paint(&bar, 1200.0);
    // Active tab 0 (x) + hovered tab 1 (x): two glyphs; the unsaved bullet
    // stays so hovering never hides unsaved work.
    assert_eq!(backend.svg_strokes.len(), close_strokes(2));
    assert_eq!(backend.round_fills.len(), 1);

    bar.hover = Some(TopBarButton::CloseDocumentTab(1));
    let backend = paint(&bar, 1200.0);
    let layout = bar.title_layout(bar_rect(1200.0), width_of).tabs.unwrap();
    assert!(
        backend
            .rect_fills
            .iter()
            .any(|(rect, color)| *rect == layout.cells[1].close_rect
                && *color == bar.theme.button_hover),
        "the hovered x gets its own wash"
    );
}

#[test]
fn active_tab_is_visually_distinct() {
    let mut bar = bar(tabs(3, 1));
    let rect = bar_rect(1200.0);
    let layout = bar.title_layout(rect, width_of).tabs.unwrap();

    let at_rest = paint(&bar, 1200.0);
    assert_eq!(
        at_rest.rect_fills,
        vec![(layout.cells[1].rect, bar.theme.row_selected)],
        "only the active tab is filled at rest"
    );

    bar.hover = Some(TopBarButton::DocumentTab(2));
    let hovered = paint(&bar, 1200.0);
    assert!(hovered
        .rect_fills
        .contains(&(layout.cells[2].rect, bar.theme.accent)));
    assert!(hovered
        .rect_fills
        .contains(&(layout.cells[1].rect, bar.theme.row_selected)));
    assert_ne!(bar.theme.row_selected, bar.theme.accent);
    assert_ne!(bar.theme.row_selected, bar.theme.background);
}

#[test]
fn status_dot_follows_agent_status() {
    let mut rows = tabs(5, 4);
    rows[0].status = DocumentTabStatus::Running;
    rows[1].status = DocumentTabStatus::NeedsReview;
    rows[2].status = DocumentTabStatus::Done;
    rows[3].status = DocumentTabStatus::Error;
    let bar = bar(rows);
    let backend = paint(&bar, 1400.0);
    let colors: Vec<_> = backend.round_fills.iter().map(|fill| fill.2).collect();
    assert_eq!(
        colors,
        vec![
            bar.theme.primary,
            bar.theme.status_warning,
            bar.theme.status_success,
            bar.theme.destructive,
        ],
        "idle tabs paint no dot"
    );
}

#[test]
fn projects_group_with_label_only_when_multiple() {
    let single = layout_document_tabs(
        &[tab(1, "alpha"), tab(2, "alpha"), tab(3, "alpha")],
        slot(1000.0),
        width_of,
    );
    assert!(single.labels.is_empty(), "one project needs no label");

    let mut rows = vec![tab(1, "alpha"), tab(2, "alpha"), tab(3, "beta")];
    rows[0].active = true;
    let grouped = layout_document_tabs(&rows, slot(1000.0), width_of);
    let names: Vec<_> = grouped.labels.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(names, ["alpha", "beta"]);
    // Each label sits directly before the first tab of its group.
    for label in &grouped.labels {
        let next = grouped
            .cells
            .iter()
            .find(|cell| cell.rect.origin.x >= label.rect.origin.x + label.rect.size.x - 0.01)
            .unwrap();
        assert!((next.rect.origin.x - (label.rect.origin.x + label.rect.size.x)).abs() < 0.01);
    }
    // Groups are told apart by space, not by a line: the gap between the two
    // groups is wider than the gap inside one.
    let inner = grouped.cells[1].rect.origin.x
        - (grouped.cells[0].rect.origin.x + grouped.cells[0].rect.size.x);
    let before_beta = grouped.labels[1].rect.origin.x
        - (grouped.cells[1].rect.origin.x + grouped.cells[1].rect.size.x);
    assert_eq!(inner, TAB_GAP);
    assert_eq!(before_beta, GROUP_GAP);

    let backend = paint(&self::bar(rows), 1200.0);
    let drawn: Vec<_> = backend.texts.iter().map(|(t, _)| t.as_str()).collect();
    assert!(drawn.contains(&"alpha") && drawn.contains(&"beta"));
}

#[test]
fn overflow_shrinks_then_scrolls() {
    // Plenty of room: tabs sit at their maximum width.
    let roomy = layout_document_tabs(&tabs(3, 0), slot(1000.0), width_of);
    assert!(roomy.more.is_none());
    assert!(roomy
        .cells
        .iter()
        .all(|cell| cell.rect.size.x == TAB_MAX_WIDTH));

    // Tighter: every tab is still shown, but narrower than the maximum and
    // never below the minimum.
    let tight = layout_document_tabs(&tabs(5, 0), slot(620.0), width_of);
    assert_eq!(tight.cells.len(), 5);
    assert!(tight.more.is_none());
    let w = tight.cells[0].rect.size.x;
    assert!((TAB_MIN_WIDTH..TAB_MAX_WIDTH).contains(&w), "shrunk to {w}");
    let end = tight.cells.last().unwrap().rect;
    assert!(end.origin.x + end.size.x <= 620.0 + 0.01);

    // Too many for the minimum width: a window that holds the active tab.
    for active in [0, 7, 19] {
        let rows = tabs(20, active);
        let layout = layout_document_tabs(&rows, slot(620.0), width_of);
        assert!(layout.cells.len() < 20);
        assert!(
            layout
                .cells
                .iter()
                .any(|cell| cell.index == active as usize),
            "active tab {active} stays visible"
        );
        let more = layout.more.as_ref().expect("+N chip");
        assert_eq!(more.hidden, 20 - layout.cells.len());
        let last = layout.cells.last().unwrap().rect;
        assert!(more.rect.origin.x >= last.origin.x + last.size.x);
        assert!(more.rect.origin.x + more.rect.size.x <= 620.0 + 0.01);
        assert!(layout
            .cells
            .iter()
            .all(|cell| cell.rect.size.x >= TAB_MIN_WIDTH - 0.01));
        // The chip stands for a hidden tab.
        assert!(layout.cell(more.target).is_none());
    }

    // The window slides: switching to a far tab brings it into view.
    let first = layout_document_tabs(&tabs(20, 0), slot(620.0), width_of);
    let moved = layout_document_tabs(&tabs(20, 19), slot(620.0), width_of);
    assert_eq!(first.cells[0].index, 0);
    assert!(moved.cells[0].index > 0);
    assert_eq!(
        moved.more.as_ref().unwrap().target,
        moved.cells[0].index - 1
    );
}

#[test]
fn overflow_chip_press_switches_to_the_first_hidden_tab() {
    let bar = bar(tabs(20, 0));
    let rect = bar_rect(900.0);
    let layout = bar.title_layout(rect, width_of).tabs.unwrap();
    let more = layout.more.as_ref().expect("+N chip");
    assert_eq!(more.target, layout.cells.len());
    assert_eq!(
        bar.hit_test_with_measure(rect, centre(more.rect), width_of),
        Some(TopBarHit::DocumentTab(more.target))
    );
    // Middle-click closes real tabs only, never the chip's stand-in.
    assert_eq!(
        bar.document_tab_index_at(rect, centre(more.rect), width_of),
        None
    );
    assert_eq!(
        bar.document_tab_index_at(rect, centre(layout.cells[1].rect), width_of),
        Some(1)
    );
}

#[test]
fn degenerate_slots_do_not_panic() {
    assert!(layout_document_tabs(&[], slot(500.0), width_of)
        .cells
        .is_empty());
    assert!(layout_document_tabs(&tabs(3, 1), slot(0.0), width_of)
        .cells
        .is_empty());
    let tiny = layout_document_tabs(&tabs(3, 1), slot(60.0), width_of);
    assert_eq!(tiny.cells.len(), 1);
    assert_eq!(tiny.cells[0].index, 1);
    assert!(tiny.cells[0].rect.size.x <= 60.0);
}

#[test]
fn tab_anchors_follow_the_layout() {
    let bar = bar(tabs(3, 0));
    let rect = bar_rect(1200.0);
    let layout = bar.title_layout(rect, width_of).tabs.unwrap();
    assert_eq!(
        bar.button_rect_with_measure(rect, TopBarButton::DocumentTab(2), width_of),
        Some(layout.cells[2].rect)
    );
    assert_eq!(
        bar.button_rect_with_measure(rect, TopBarButton::CloseDocumentTab(2), width_of),
        Some(layout.cells[2].close_rect)
    );
    assert_eq!(
        bar.button_rect_with_measure(rect, TopBarButton::DocumentTab(9), width_of),
        None
    );
    let hidden = TopBar::new("x");
    assert_eq!(
        hidden.button_rect_with_measure(rect, TopBarButton::DocumentTab(0), width_of),
        None
    );
}

#[test]
fn tabs_carry_no_tooltip_but_the_close_button_does() {
    use crate::widgets::top_bar_tooltip::{suppressed, tooltip_for};
    let ui = EditorUiState::default();
    assert!(suppressed(&ui, TopBarButton::DocumentTab(0)));
    assert_eq!(tooltip_for(&ui, TopBarButton::DocumentTab(0)).label, "");
    assert!(!suppressed(&ui, TopBarButton::CloseDocumentTab(0)));
    assert_eq!(
        tooltip_for(&ui, TopBarButton::CloseDocumentTab(0)).label,
        crate::widgets::editor_state_ext::translate(&ui, "common.close")
    );
}

#[test]
fn a_tab_too_narrow_for_its_close_button_has_none() {
    // Even one tab cannot get its minimum width, so the fallback window is
    // squeezed; its close button must not spill past the tab.
    let layout = layout_document_tabs(&tabs(3, 1), slot(40.0), width_of);
    for cell in &layout.cells {
        assert_eq!(cell.close_rect.size.x, 0.0, "no room for a close button");
        assert_eq!(
            layout.hit(Point2D::new(cell.rect.origin.x + 1.0, 20.0)),
            Some(TopBarHit::DocumentTab(cell.index))
        );
    }
    let roomy = layout_document_tabs(&tabs(2, 0), slot(1000.0), width_of);
    assert!(roomy.cells[0].close_rect.size.x > 0.0);
}
