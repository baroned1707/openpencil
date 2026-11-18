use super::*;
use crate::design_styles::style_records;

#[test]
fn every_embedded_style_renders_within_the_cap_with_the_precedence_marker() {
    let records = style_records();
    if records.is_empty() {
        return; // wasm32 builds embed no style table
    }
    for record in records {
        let text = chosen_style_paragraph(&record.reference).expect("a known style renders");
        assert!(
            text.len() <= MAX_CHOSEN_STYLE_BYTES,
            "{}: {}",
            record.reference,
            text.len()
        );
        assert!(
            text.starts_with(CHOSEN_STYLE_MARKER),
            "{}",
            record.reference
        );
        assert!(text.contains(&record.name), "{}", record.reference);
        assert!(!text.contains('\u{0}'));
    }
}

#[test]
fn the_marker_says_what_outranks_the_choice() {
    assert!(CHOSEN_STYLE_MARKER.contains("design.md"));
    assert!(CHOSEN_STYLE_MARKER.contains("pinned style"));
    assert!(CHOSEN_STYLE_MARKER.contains("outrank"));
}

#[test]
fn the_paragraph_carries_use_notes_and_colours_when_the_record_has_them() {
    let Some(record) = style_records().iter().find(|r| {
        r.best_for.is_some() && r.do_not_use_for.is_some() && !r.primary_colors.is_empty()
    }) else {
        return;
    };
    let text = chosen_style_paragraph(&record.reference).expect("renders");
    assert!(text.contains("Best for:"));
    assert!(text.contains("Do not use for:"));
    assert!(text.contains(&record.primary_colors[0]));
}

#[test]
fn an_unknown_style_renders_nothing() {
    assert_eq!(chosen_style_paragraph("no-such-style-anywhere"), None);
}
