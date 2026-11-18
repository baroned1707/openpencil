use super::*;
use crate::document_tabs::{DocumentId, DocumentTabStatus, DocumentTabView};

fn ui_with_tabs(count: u64, active: u64) -> EditorUiState {
    let mut ui = EditorUiState {
        document_tabs_supported: true,
        ..EditorUiState::default()
    };
    ui.document_tabs = (0..count)
        .map(|n| DocumentTabView {
            id: DocumentId(100 + n),
            label: format!("doc-{n}.op"),
            project: String::new(),
            dirty: false,
            status: DocumentTabStatus::Idle,
            active: n == active,
        })
        .collect();
    ui
}

#[test]
fn shortcut_ctrl_digit_requests_index() {
    let ui = ui_with_tabs(9, 0);
    assert_eq!(
        document_tab_shortcut(&ui, "1", false, false),
        Some(DocumentTabRequest::SwitchToIndex(0))
    );
    assert_eq!(
        document_tab_shortcut(&ui, "9", false, false),
        Some(DocumentTabRequest::SwitchToIndex(8))
    );
    let few = ui_with_tabs(3, 0);
    assert_eq!(
        document_tab_shortcut(&few, "4", false, false),
        None,
        "no tab at that position"
    );
    assert_eq!(
        document_tab_shortcut(&few, "9", false, false),
        Some(DocumentTabRequest::SwitchToIndex(2)),
        "9 is always the last tab"
    );
    assert_eq!(document_tab_shortcut(&ui, "0", false, false), None);
    assert_eq!(document_tab_shortcut(&ui, "a", false, false), None);
    assert_eq!(document_tab_shortcut(&ui, "12", false, false), None);
    // Shift+digit is a different chord (it types a symbol).
    assert_eq!(document_tab_shortcut(&ui, "1", true, false), None);
}

#[test]
fn shortcut_brackets_cycle_and_wrap() {
    let ui = ui_with_tabs(3, 0);
    assert_eq!(
        document_tab_shortcut(&ui, "]", true, false),
        Some(DocumentTabRequest::Switch(DocumentId(101)))
    );
    assert_eq!(
        document_tab_shortcut(&ui, "[", true, false),
        Some(DocumentTabRequest::Switch(DocumentId(102))),
        "previous from the first tab wraps to the last"
    );
    let last = ui_with_tabs(3, 2);
    assert_eq!(
        document_tab_shortcut(&last, "}", true, false),
        Some(DocumentTabRequest::Switch(DocumentId(100))),
        "next from the last tab wraps to the first"
    );
    // Without Shift the bracket keys keep their canvas meaning (reorder).
    assert_eq!(document_tab_shortcut(&ui, "[", false, false), None);
}

#[test]
fn shortcut_ignored_while_text_input_focused() {
    let ui = ui_with_tabs(3, 0);
    assert_eq!(document_tab_shortcut(&ui, "2", false, true), None);
    assert_eq!(document_tab_shortcut(&ui, "]", true, true), None);
}

#[test]
fn shortcut_noop_when_tabs_unsupported() {
    let mut ui = ui_with_tabs(3, 0);
    ui.document_tabs_supported = false;
    assert_eq!(document_tab_shortcut(&ui, "2", false, false), None);
    assert_eq!(document_tab_shortcut(&ui, "]", true, false), None);
}

#[test]
fn shortcut_noop_with_a_single_document() {
    let ui = ui_with_tabs(1, 0);
    assert_eq!(document_tab_shortcut(&ui, "1", false, false), None);
    assert_eq!(document_tab_shortcut(&ui, "]", true, false), None);
}

#[test]
fn cmd_w_closes_the_active_tab_only_when_it_has_a_meaning() {
    let ui = ui_with_tabs(3, 1);
    assert_eq!(
        close_document_tab_shortcut(&ui, false),
        Some(DocumentTabRequest::Close(DocumentId(101)))
    );
    assert_eq!(close_document_tab_shortcut(&ui, true), None, "typing");
    assert_eq!(
        close_document_tab_shortcut(&ui_with_tabs(1, 0), false),
        None
    );
    let mut unsupported = ui_with_tabs(3, 0);
    unsupported.document_tabs_supported = false;
    assert_eq!(close_document_tab_shortcut(&unsupported, false), None);
}

/// The dock's open mark is the chord that really reaches the tab: for every
/// tab count and position, the badge digit raises `SwitchToIndex(index)`, and a
/// tab with no badge has no digit chord to it.
#[test]
fn badge_digit_is_the_chord_that_reaches_the_tab() {
    for count in 1..=14_u64 {
        let ui = ui_with_tabs(count, 0);
        for index in 0..count as usize {
            let reached: Vec<u8> = (1..=9_u8)
                .filter(|digit| {
                    document_tab_shortcut(&ui, &digit.to_string(), false, false)
                        == Some(DocumentTabRequest::SwitchToIndex(index))
                })
                .collect();
            match shortcut_digit_for(count as usize, index) {
                Some(digit) => assert!(
                    reached.contains(&digit),
                    "count {count} index {index}: badge {digit} does not reach it ({reached:?})"
                ),
                None => assert!(
                    reached.is_empty(),
                    "count {count} index {index}: no badge but digits {reached:?} reach it"
                ),
            }
        }
    }
}

#[test]
fn badge_digits_stop_at_eight_then_nine_marks_only_the_last_tab() {
    assert_eq!(shortcut_digit_for(1, 0), None, "no chord with one tab");
    assert_eq!(shortcut_digit_for(2, 1), Some(2));
    assert_eq!(shortcut_digit_for(9, 8), Some(9));
    assert_eq!(shortcut_digit_for(12, 7), Some(8));
    assert_eq!(shortcut_digit_for(12, 8), None, "beyond 8 and not last");
    assert_eq!(shortcut_digit_for(12, 10), None);
    assert_eq!(shortcut_digit_for(12, 11), Some(9), "9 is the last tab");
    assert_eq!(shortcut_digit_for(3, 3), None, "past the end");
}
