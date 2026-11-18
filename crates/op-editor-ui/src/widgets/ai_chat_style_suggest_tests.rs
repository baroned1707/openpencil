use super::*;

fn row(width: f32) -> Rect {
    Rect::xywh(0.0, 100.0, width, CHIP_ROW_HEIGHT)
}

fn assert_in_row(row: Rect, rect: Rect) {
    assert!(
        rect.origin.x >= row.origin.x - 1e-3,
        "{rect:?} left of {row:?}"
    );
    assert!(
        rect.origin.x + rect.size.x <= row.origin.x + row.size.x + 1e-3,
        "{rect:?} right of {row:?}"
    );
    assert!(
        rect.origin.y >= row.origin.y && rect.origin.y + rect.size.y <= row.origin.y + row.size.y
    );
}

fn assert_ordered(layout: &SuggestRowLayout) {
    let mut previous_end = layout.lead.origin.x + layout.lead.size.x;
    for chip in &layout.chips {
        assert!(
            chip.origin.x >= previous_end - 1e-3,
            "chips overlap: {layout:?}"
        );
        previous_end = chip.origin.x + chip.size.x;
    }
    assert!(layout.dismiss.origin.x >= previous_end - 1e-3, "{layout:?}");
}

#[test]
fn everything_keeps_its_natural_width_when_it_fits() {
    let row = row(520.0);
    let layout = layout_row(row, 90.0, &[100.0, 110.0, 60.0]);
    assert_eq!(layout.chips.len(), 3);
    assert!((layout.lead.size.x - 90.0).abs() < 1e-3);
    assert!(!layout.lead_is_icon);
    for (chip, natural) in layout.chips.iter().zip([100.0, 110.0, 60.0]) {
        assert!((chip.size.x - natural).abs() < 1e-3);
        assert_in_row(row, *chip);
    }
    assert_in_row(row, layout.dismiss);
    assert_ordered(&layout);
}

#[test]
fn one_long_name_cannot_take_the_row() {
    let layout = layout_row(row(520.0), 90.0, &[400.0]);
    assert!((layout.chips[0].size.x - CHIP_MAX_W).abs() < 1e-3);
}

#[test]
fn the_rail_width_swaps_the_label_for_a_glyph_and_keeps_two_readable_chips() {
    // 288 px: the rail's panel row. Three long names plus the label cannot all
    // be readable: the label is replaced (never cut) by the palette glyph, and
    // the chips stay wide enough to recognise.
    let row = row(288.0);
    let layout = layout_row(row, 90.0, &[100.0, 113.0, 128.0]);
    assert!(layout.lead_is_icon, "{layout:?}");
    assert!((layout.lead.size.x - LEAD_ICON).abs() < 1e-3);
    assert_eq!(layout.chips.len(), 2, "{layout:?}");
    for chip in &layout.chips {
        assert!(chip.size.x >= CHIP_MIN_W - 1e-3);
        assert_in_row(row, *chip);
    }
    assert_ordered(&layout);
}

#[test]
fn a_mid_width_keeps_the_label_with_two_chips() {
    let row = row(328.0);
    let layout = layout_row(row, 90.0, &[100.0, 113.0, 128.0]);
    assert!((layout.lead.size.x - 90.0).abs() < 1e-3, "{layout:?}");
    assert!(!layout.lead_is_icon);
    assert_eq!(layout.chips.len(), 2);
    assert_ordered(&layout);
    for chip in &layout.chips {
        assert_in_row(row, *chip);
    }
}

#[test]
fn short_names_keep_their_own_small_width() {
    let layout = layout_row(row(300.0), 90.0, &[50.0, 50.0, 50.0]);
    assert_eq!(layout.chips.len(), 3);
    for chip in &layout.chips {
        assert!((chip.size.x - 50.0).abs() < 1e-3);
    }
}

#[test]
fn a_very_narrow_row_never_overlaps_and_never_leaves_the_row() {
    for width in [30.0, 60.0, 100.0, 140.0, 180.0, 240.0] {
        let tight = row(width);
        let layout = layout_row(tight, 90.0, &[100.0, 100.0, 100.0]);
        for chip in &layout.chips {
            assert_in_row(tight, *chip);
        }
        assert_in_row(tight, layout.dismiss);
        assert_ordered(&layout);
    }
}

#[test]
fn no_chips_means_no_lead() {
    let layout = layout_row(row(40.0), 90.0, &[100.0]);
    assert!(layout.chips.is_empty());
    assert_eq!(layout.lead.size.x, 0.0);
}
