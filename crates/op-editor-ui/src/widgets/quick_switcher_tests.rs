use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;
use op_editor_core::quick_switcher::QuickSwitcherTarget;
use op_editor_core::DocumentId;

const TEXT: QuickSwitcherText<'static> = QuickSwitcherText {
    placeholder: "Go to a file or project",
    empty: "No matching files",
};

fn row(label: &str, project: &str, open: bool, digit: Option<u8>) -> QuickSwitcherRow {
    QuickSwitcherRow {
        target: QuickSwitcherTarget::Document(DocumentId(1)),
        label: label.to_string(),
        project: project.to_string(),
        dirty: false,
        open,
        digit,
    }
}

#[test]
fn the_card_is_centred_below_the_top_bar_and_inside_a_narrow_window() {
    let layout = QuickSwitcherLayout::new(5, 0, 1440.0);
    assert_eq!(layout.card.size.x, CARD_WIDTH);
    assert!((layout.card.origin.x * 2.0 + layout.card.size.x - 1440.0).abs() < 0.01);
    assert!(layout.card.origin.y >= TOP_BAR_HEIGHT, "below the top bar");
    let narrow = QuickSwitcherLayout::new(5, 0, 300.0);
    assert!(narrow.card.origin.x >= 0.0 && narrow.card.size.x <= 300.0);
}

#[test]
fn at_most_eight_rows_show_and_the_window_starts_at_first_visible() {
    let layout = QuickSwitcherLayout::new(50, 0, 1440.0);
    assert_eq!(layout.rows.len(), VISIBLE_ROWS);
    assert_eq!(layout.rows[0].0, 0);
    let scrolled = QuickSwitcherLayout::new(50, 20, 1440.0);
    assert_eq!(scrolled.rows[0].0, 20);
    assert_eq!(scrolled.rows.last().unwrap().0, 20 + VISIBLE_ROWS - 1);
    // A stale first_visible past the end is clamped, never an empty window.
    let stale = QuickSwitcherLayout::new(10, 99, 1440.0);
    assert_eq!(stale.rows.len(), VISIBLE_ROWS);
    assert_eq!(stale.rows[0].0, 10 - VISIBLE_ROWS);
    // Rows stack inside the card with no gap.
    for pair in layout.rows.windows(2) {
        assert_eq!(pair[0].1.origin.y + pair[0].1.size.y, pair[1].1.origin.y);
    }
    let last = layout.rows.last().unwrap().1;
    assert!(last.origin.y + last.size.y < layout.card.origin.y + layout.card.size.y);
}

#[test]
fn no_rows_shows_the_empty_line_and_still_has_a_query_line() {
    let layout = QuickSwitcherLayout::new(0, 0, 1440.0);
    assert!(layout.empty && layout.rows.is_empty());
    assert!(layout.card.contains(Point2D::new(
        layout.input.origin.x + 1.0,
        layout.input.origin.y + 1.0
    )));
}

#[test]
fn hit_testing_reports_the_row_the_card_or_outside() {
    let layout = QuickSwitcherLayout::new(3, 0, 1440.0);
    let (index, rect) = layout.rows[1];
    let centre = Point2D::new(rect.origin.x + 10.0, rect.origin.y + rect.size.y / 2.0);
    assert_eq!(layout.hit(centre), QuickSwitcherHit::Row(index));
    let on_query = Point2D::new(layout.input.origin.x + 5.0, layout.input.origin.y + 5.0);
    assert_eq!(layout.hit(on_query), QuickSwitcherHit::Inside);
    assert_eq!(
        layout.hit(Point2D::new(2.0, 2.0)),
        QuickSwitcherHit::Outside
    );
}

fn paint_texts(rows: &[QuickSwitcherRow], selected: usize, query: &str) -> CaptureBackend {
    let mut state = QuickSwitcherState::default();
    state.open_fresh(0);
    state.input.set_text(query);
    state.selected = selected;
    let layout = QuickSwitcherLayout::new(rows.len(), 0, 1440.0);
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint(&mut cx, &Theme::dark(), &layout, &state, rows, &TEXT, 0);
    backend
}

#[test]
fn rows_paint_name_project_and_the_open_mark() {
    let rows = vec![
        row("home.op", "shop", true, Some(1)),
        row("scratch.op", "", true, None),
        row("pricing.op", "shop", false, None),
    ];
    let painted = paint_texts(&rows, 0, "");
    let texts: Vec<&str> = painted.texts.iter().map(|(t, _)| t.as_str()).collect();
    for expected in ["home.op", "scratch.op", "pricing.op", "shop", "\u{2318}1"] {
        assert!(texts.contains(&expected), "{expected} missing in {texts:?}");
    }
    assert!(
        !texts.contains(&"\u{2318}2"),
        "no digit invented for a row without one"
    );
    assert!(
        painted
            .texts
            .iter()
            .any(|(t, _)| t == "Go to a file or project"),
        "the placeholder shows while the query is empty"
    );
    // The open dot for the document no chord reaches (4 px), none for closed files.
    let dots = painted
        .round_fills
        .iter()
        .filter(|(r, _, _)| (r.size.x - 4.0).abs() < 0.01)
        .count();
    assert_eq!(dots, 1);
}

#[test]
fn only_the_selected_row_gets_the_wash() {
    let rows = vec![row("a.op", "", false, None), row("b.op", "", false, None)];
    let theme = Theme::dark();
    let painted = paint_texts(&rows, 1, "");
    let washes: Vec<_> = painted
        .rect_fills
        .iter()
        .filter(|(_, color)| *color == theme.row_selected_primary)
        .collect();
    assert_eq!(washes.len(), 1);
    let layout = QuickSwitcherLayout::new(2, 0, 1440.0);
    assert_eq!(washes[0].0, layout.rows[1].1);
}

#[test]
fn an_empty_result_paints_the_no_matches_line() {
    let painted = paint_texts(&[], 0, "zzz");
    assert!(painted.texts.iter().any(|(t, _)| t == "No matching files"));
    assert!(
        painted.texts.iter().any(|(t, _)| t == "zzz"),
        "the query shows"
    );
}

#[test]
fn a_long_name_is_ellipsized_before_it_reaches_the_mark() {
    let long = "an-extremely-long-design-file-name-for-the-checkout-flow-and-more-and-more.op";
    let rows = vec![row(long, "shop", true, Some(1))];
    let painted = paint_texts(&rows, 0, "");
    let label = painted
        .texts
        .iter()
        .map(|(t, _)| t.as_str())
        .find(|t| t.starts_with("an-extremely"))
        .expect("label painted");
    assert!(label.len() < long.len(), "ellipsized: {label}");
    assert!(
        !painted.texts.iter().any(|(t, _)| t == "shop"),
        "the project gives way first"
    );
}
