use super::*;
use op_i18n::Locale;

#[test]
fn locale_choice_stays_on_the_old_language_until_the_catalog_is_ready() {
    let mut ui = op_editor_core::editor_ui_state::EditorUiState::new();
    assert_eq!(ui.locale, Locale::EnUs);

    assert!(select_locale(&mut ui, Locale::De, false));
    assert_eq!(ui.locale, Locale::EnUs);
    assert_eq!(ui.pending_locale, Some(Locale::De));

    assert!(!select_locale(&mut ui, Locale::De, true));
    assert_eq!(ui.locale, Locale::De);
    assert_eq!(ui.pending_locale, None);
}

#[test]
fn choosing_a_ready_locale_cancels_an_older_pending_choice() {
    let mut ui = op_editor_core::editor_ui_state::EditorUiState::new();
    ui.pending_locale = Some(Locale::Ja);
    ui.locale_persistence_override = Some(Locale::Ja);

    assert!(!select_locale(&mut ui, Locale::EnUs, true));
    assert_eq!(ui.locale, Locale::EnUs);
    assert_eq!(ui.pending_locale, None);
    assert_eq!(ui.locale_persistence_override, None);
}

#[test]
fn changing_a_partition_pending_choice_preserves_the_stored_locale() {
    let mut ui = op_editor_core::editor_ui_state::EditorUiState::new();
    ui.pending_locale = Some(Locale::Ja);
    ui.locale_persistence_override = Some(Locale::Ja);

    assert!(select_locale(&mut ui, Locale::De, false));
    assert_eq!(ui.locale, Locale::EnUs);
    assert_eq!(ui.pending_locale, Some(Locale::De));
    assert_eq!(ui.locale_persistence_override, Some(Locale::Ja));
}

mod page_switch_parity {
    //! The left rail's page rows and the info bar's page chip share
    //! `switch_page`: the same press must leave the same state behind.
    use super::*;
    use op_editor_core::{switch_page, EditorState, NodeId, PageSwitch};

    fn state(active: usize) -> EditorState {
        let mut state = EditorState::default();
        while state.page_count() < 3 {
            state.add_page().expect("page added");
        }
        state.ui.active_page_index = active;
        state.set_single_selection(NodeId::new("n1"));
        state
    }

    fn snapshot(state: &EditorState) -> (usize, bool) {
        (state.ui.active_page_index, state.selection.anchor.is_real())
    }

    #[test]
    fn the_rail_row_and_the_helper_agree_for_every_page() {
        for active in 0..3 {
            for target in 0..3 {
                let mut rail = state(active);
                let outcome =
                    apply_layer_panel_click(&mut rail, LayerPanelHit::Page(target), 1_000, false);
                let mut helper = state(active);
                let switched = switch_page(&mut helper, target);

                assert_eq!(snapshot(&rail), snapshot(&helper), "{active}->{target}");
                let expected = match switched {
                    PageSwitch::Refit => LayerPanelClick::Refit,
                    PageSwitch::Preserved => LayerPanelClick::Dirty,
                };
                assert_eq!(outcome, expected, "{active}->{target}");
            }
        }
    }
}
