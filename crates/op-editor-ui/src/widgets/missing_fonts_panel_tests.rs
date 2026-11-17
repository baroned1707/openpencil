use super::*;
use crate::widgets::missing_fonts_row::{family_label_lines, row_text_rect, TEXT_BUTTON_GAP};
use crate::widgets::property_panel_typography::FontPickerRow;
use op_editor_core::missing_fonts::MissingFontEntry;

fn prompt_state(rows: usize) -> EditorState {
    let mut state = EditorState::new();
    state.editor_ui.missing_fonts_prompt = Some(MissingFontsPrompt {
        entries: (0..rows)
            .map(|row| MissingFontEntry {
                family: format!("F{row}"),
                run_count: 1,
                mismatch_note: None,
                resolved: false,
            })
            .collect(),
    });
    state.editor_ui.missing_fonts_modal_open = true;
    state
}

fn viewport() -> Rect {
    Rect::xywh(0.0, 0.0, 1200.0, 800.0)
}

#[test]
fn long_css_stack_wraps_before_action_without_losing_text() {
    let state = prompt_state(1);
    let row = Rect::xywh(20.0, 30.0, 440.0, ROW_HEIGHT);
    let action = row_button_rect(row, &state.editor_ui);
    let entry = state
        .editor_ui
        .missing_fonts_prompt
        .as_ref()
        .unwrap()
        .entries[0]
        .clone();
    let text_rect = row_text_rect(row, &state.editor_ui, &entry);
    let family = "Inter,ui-sans-serif,system-ui,-apple-system,\"PingFang SC\",sans-serif";
    let measure = |text: &str| text.chars().count() as f32 * 7.0;
    let lines = family_label_lines(family, text_rect.size.x, measure);

    assert_eq!(lines.len(), 2, "the long stack should use both lines");
    assert_eq!(
        lines.concat(),
        family,
        "a two-line fit should stay complete"
    );
    assert!(lines.iter().all(|line| measure(line) <= text_rect.size.x));
    assert!(
        text_rect.origin.x + text_rect.size.x + TEXT_BUTTON_GAP <= action.origin.x,
        "text and action gutters must not overlap"
    );
}

#[test]
fn exceptionally_long_family_ellipsizes_on_second_line() {
    let family = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let measure = |text: &str| text.chars().count() as f32 * 10.0;
    let lines = family_label_lines(family, 100.0, measure);

    assert_eq!(lines.len(), 2);
    assert!(lines[1].ends_with('…'));
    assert!(lines.iter().all(|line| measure(line) <= 100.0));
}

#[test]
fn action_hit_box_is_vertically_centered_in_the_shared_row() {
    let state = prompt_state(1);
    let row = Rect::xywh(20.0, 30.0, 440.0, ROW_HEIGHT);
    let action = row_button_rect(row, &state.editor_ui);

    assert_eq!(
        action.origin.y,
        row.origin.y + (ROW_HEIGHT - action.size.y) / 2.0
    );
}

#[test]
fn choose_file_button_hit_maps_to_row() {
    let state = prompt_state(2);
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);
    let button = panel.row_button_rect(rect, 1);
    let centre = Point2D::new(
        button.origin.x + button.size.x / 2.0,
        button.origin.y + button.size.y / 2.0,
    );

    assert_eq!(
        panel.hit_test(rect, viewport(), centre),
        MissingFontsHit::ChooseFont(1)
    );
}

#[test]
fn outside_click_does_not_choose() {
    let state = prompt_state(1);
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);

    assert_eq!(
        panel.hit_test(rect, viewport(), Point2D::new(1.0, 1.0)),
        MissingFontsHit::Outside
    );
}

#[test]
fn resolved_row_has_no_button() {
    let mut state = prompt_state(1);
    state
        .editor_ui
        .missing_fonts_prompt
        .as_mut()
        .expect("prompt")
        .entries[0]
        .resolved = true;
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);
    let button = panel.row_button_rect(rect, 0);
    let centre = Point2D::new(
        button.origin.x + button.size.x / 2.0,
        button.origin.y + button.size.y / 2.0,
    );

    assert_eq!(
        panel.hit_test(rect, viewport(), centre),
        MissingFontsHit::Inside
    );
}

#[test]
fn dismiss_button_has_a_dedicated_hit() {
    let state = prompt_state(1);
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);
    let dismiss = dismiss_rect(rect, &state.editor_ui);
    let centre = Point2D::new(
        dismiss.origin.x + dismiss.size.x / 2.0,
        dismiss.origin.y + dismiss.size.y / 2.0,
    );

    assert_eq!(
        panel.hit_test(rect, viewport(), centre),
        MissingFontsHit::Dismiss
    );
}

#[test]
fn long_prompt_is_viewport_bounded_with_fixed_dismiss_and_scrollable_rows() {
    let mut state = prompt_state(20);
    let (rect, max_scroll) = {
        let panel = MissingFontsPanel::for_editor(&state).expect("open");
        let rect = panel.rect(600.0, 400.0);
        assert!(rect.origin.y + rect.size.y <= 392.0);
        assert!(panel.max_rows_scroll(rect) > 0.0);
        (rect, panel.max_rows_scroll(rect))
    };

    state.editor_ui.missing_fonts_scroll.offset = max_scroll;
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let last = panel.row_button_rect(rect, 19);
    let last_center = Point2D::new(
        last.origin.x + last.size.x / 2.0,
        last.origin.y + last.size.y / 2.0,
    );
    assert_eq!(
        panel.hit_test(rect, Rect::xywh(0.0, 0.0, 600.0, 400.0), last_center),
        MissingFontsHit::ChooseFont(19)
    );

    let dismiss = dismiss_rect(rect, &state.editor_ui);
    assert!(dismiss.origin.y + dismiss.size.y <= 400.0);
}

#[test]
fn shared_picker_selects_a_system_font_and_exposes_import() {
    let mut state = prompt_state(1);
    state.editor_ui.system_font_families = std::sync::Arc::new(vec!["Arial".to_string()]);
    state.editor_ui.font_import_supported = true;
    state
        .editor_ui
        .open_missing_font_picker(0, op_editor_core::MissingFontSurface::Prompt);
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);
    let layout = panel.picker_layout(rect, viewport()).expect("picker");
    let entries = panel.picker_entries();
    let arial = entries
        .iter()
        .position(|entry| entry.family == "Arial")
        .expect("Arial entry");
    let arial_rect = layout
        .rows
        .iter()
        .find_map(|(row, rect)| {
            matches!(row, FontPickerRow::Entry(index) if *index == arial).then_some(*rect)
        })
        .expect("Arial row");
    let arial_point = Point2D::new(
        arial_rect.origin.x + 12.0,
        arial_rect.origin.y + arial_rect.size.y / 2.0,
    );
    assert_eq!(
        panel.hit_test(rect, viewport(), arial_point),
        MissingFontsHit::SelectFont(arial)
    );

    let import_rect = layout
        .rows
        .iter()
        .find_map(|(row, rect)| matches!(row, FontPickerRow::ImportAction).then_some(*rect))
        .expect("import row");
    let import_point = Point2D::new(
        import_rect.origin.x + 12.0,
        import_rect.origin.y + import_rect.size.y / 2.0,
    );
    assert_eq!(
        panel.hit_test(rect, viewport(), import_point),
        MissingFontsHit::ImportFont(0)
    );
}

#[test]
fn click_outside_shared_picker_closes_only_the_picker() {
    let mut state = prompt_state(1);
    state
        .editor_ui
        .open_missing_font_picker(0, op_editor_core::MissingFontSurface::Prompt);
    let panel = MissingFontsPanel::for_editor(&state).expect("open");
    let rect = panel.rect(1200.0, 800.0);

    assert_eq!(
        panel.hit_test(rect, viewport(), Point2D::new(4.0, 4.0)),
        MissingFontsHit::ClosePicker
    );
}

#[test]
fn closed_or_empty_prompt_builds_none() {
    let state = EditorState::new();
    assert!(MissingFontsPanel::for_editor(&state).is_none());

    let mut state = prompt_state(0);
    assert!(MissingFontsPanel::for_editor(&state).is_none());
    state.editor_ui.missing_fonts_prompt = Some(MissingFontsPrompt {
        entries: vec![MissingFontEntry {
            family: "Katibeh".into(),
            run_count: 1,
            mismatch_note: None,
            resolved: false,
        }],
    });
    state.editor_ui.missing_fonts_modal_open = false;
    assert!(MissingFontsPanel::for_editor(&state).is_none());
}
