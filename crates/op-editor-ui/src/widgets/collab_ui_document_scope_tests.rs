//! The collaboration chip and panel show the session of the document that is
//! on screen, not one app-wide session.

use super::*;
use crate::widgets::collab_panel::CollabPanel;
use op_editor_core::{AuthenticatedCollabSession, DocumentScope, EditorState};

fn session_in_document_a() -> EditorState {
    let mut state = EditorState::starter();
    let collab = &mut state.editor_ui.collab;
    collab.availability = CollabAvailability::Ready;
    collab.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: "Landing page".to_string(),
            role: CollabUiRole::Editor,
            share_endpoint: None,
        },
        vec![CollabParticipantUi::new(
            "p1",
            "Ada",
            0x3366ffff,
            CollabUiRole::Editor,
            false,
        )],
    );
    collab.panel.open = true;
    state
}

#[test]
fn top_bar_chip_follows_the_on_screen_documents_session() {
    let mut live = session_in_document_a();
    let mut document_b = DocumentScope::from_state(EditorState::starter());

    let chip = CollabTopBarModel::for_editor_ui(&live.editor_ui);
    assert_eq!(chip.tone, CollabTopBarTone::Connected);
    assert_eq!(chip.avatars.len(), 1);
    assert!(CollabPanel::for_editor_ui(&live.editor_ui).is_some());

    // Viewing B: the chip is the neutral "Collaborate" button (availability is
    // the app's, so it is still offered) and the panel is closed.
    live.switch_document_scope(&mut document_b);
    let chip = CollabTopBarModel::for_editor_ui(&live.editor_ui);
    assert!(
        chip.visible && chip.enabled,
        "collaboration is still offered"
    );
    assert_eq!(chip.tone, CollabTopBarTone::Neutral);
    assert_eq!(
        chip.label,
        op_i18n::translate(
            live.editor_ui.effective_locale(),
            "collab.topbar.collaborate"
        )
    );
    assert!(chip.avatars.is_empty());
    assert!(CollabPanel::for_editor_ui(&live.editor_ui).is_none());

    // Back on A, its session and panel are as they were.
    live.switch_document_scope(&mut document_b);
    let chip = CollabTopBarModel::for_editor_ui(&live.editor_ui);
    assert_eq!(chip.tone, CollabTopBarTone::Connected);
    assert_eq!(chip.avatars.len(), 1);
    assert!(CollabPanel::for_editor_ui(&live.editor_ui).is_some());
}
