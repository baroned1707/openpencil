//! The session display state travels with a join, and with the user's tab
//! changes only where it must.
//!
//! The state is per document (`DocumentScope`), but a join opens its own tab,
//! so the queued action, the typed address and the open panel that started it
//! must follow into that tab, and back if the join falls through.

use super::*;
use op_editor_core::{CollabAvailability, CollabPanelView, CollabUiState, DocumentId};

/// An app whose open document has content, so a join cannot reuse it.
fn app_with_work() -> DesktopApp {
    let mut app = DesktopApp::new(None);
    app.host.editor_state_mut().mark_document_changed();
    assert!(!app.active_document_is_pristine_untitled());
    app
}

/// What the join panel looks like after the user typed an address and pressed
/// Join: panel open on the join view, address in the field, the action queued.
fn press_join(app: &mut DesktopApp, address: &str) {
    let collab = &mut app.host.editor_state_mut().editor_ui.collab;
    collab.panel.open = true;
    collab.panel.view = CollabPanelView::Join;
    collab.panel.join_input.set_text(address);
    collab.panel.join_address_focused = true;
    collab.pending_action = Some(CollabUiAction::JoinAddress {
        endpoint: address.into(),
    });
}

/// Inspect the session display of a tab that is not on screen.
fn parked_session<R>(
    app: &mut DesktopApp,
    id: DocumentId,
    read: impl FnOnce(&CollabUiState) -> R,
) -> R {
    let index = app.documents.index_of(id).expect("tab is open");
    let parked = app.documents.take_parked(index).expect("tab is parked");
    let result = read(parked.scope().collab());
    app.documents.restore_parked(index, parked);
    result
}

/// The runtime step of a join that never gets further than `Joining`.
fn start_join(app: &mut DesktopApp) -> (Option<String>, bool, bool, DocumentId) {
    let mut seen = (None, false, false, app.documents.active_id());
    app.pump_collaboration_transition_with(false, |app| {
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        let action_present = matches!(
            collab.pending_action,
            Some(CollabUiAction::JoinAddress { .. })
        );
        seen = (
            Some(collab.panel.join_input.text().to_string()),
            collab.panel.open && collab.panel.join_address_focused,
            action_present,
            app.documents.active_id(),
        );
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        let _ = collab.take_pending_action();
        collab.set_phase(CollabConnectionPhase::Joining);
        true
    });
    seen
}

#[test]
fn join_input_and_pending_action_move_to_the_new_tab() {
    let mut app = app_with_work();
    let original = app.documents.active_id();
    press_join(&mut app, "10.0.0.9:7");

    let (typed, panel_open_and_focused, action_present, active) = start_join(&mut app);

    // The runtime step ran in the NEW tab and found the whole request there.
    assert_ne!(active, original);
    assert_eq!(app.documents.len(), 2);
    assert_eq!(typed.as_deref(), Some("10.0.0.9:7"));
    assert!(
        panel_open_and_focused,
        "panel and focus moved with the join"
    );
    assert!(action_present, "the queued join action moved with the join");

    // The old tab was left with an idle session.
    parked_session(&mut app, original, |collab| {
        assert!(collab.pending_action.is_none());
        assert!(collab.panel.join_input.text().is_empty());
        assert!(!collab.panel.join_address_focused);
        assert!(!collab.panel.open);
        assert_eq!(collab.phase, CollabConnectionPhase::Idle);
    });
    // The new tab shows the join in progress.
    let live = &app.host.editor_state().editor_ui.collab;
    assert_eq!(live.phase, CollabConnectionPhase::Joining);
    assert!(live.panel.open);
}

#[test]
fn a_failed_join_gives_its_notice_and_panel_back_to_the_original_tab() {
    // The REAL runtime refuses the join (the open-source build has no ticket
    // issuer) and reports why in the session's notice, which would otherwise
    // be destroyed together with the unused tab.
    let mut app = app_with_work();
    let original = app.documents.active_id();
    press_join(&mut app, "10.0.0.9:7");

    assert!(app.pump_collaboration_transition());
    assert_eq!(app.documents.len(), 2, "the join opened its own tab");
    let _ = app.collab_runtime.poll(&mut app.host);
    assert!(app.settle_guest_join_tab());

    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), original);
    let collab = &app.host.editor_state().editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Idle);
    assert!(
        collab.notice.is_some(),
        "the user is told why the join failed"
    );
    assert!(collab.panel.open, "the panel is still open");
    assert_eq!(
        collab.panel.join_input.text(),
        "10.0.0.9:7",
        "the address is kept"
    );
}

#[test]
fn discovery_is_not_bound_to_a_tab_so_it_follows_the_user() {
    let mut app = app_with_work();
    assert!(app.open_untitled_tab());
    let first = app.documents.id_at(0).expect("first tab");
    {
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        collab.set_phase(CollabConnectionPhase::Discovering);
        collab.panel.open = true;
        collab.panel.view = CollabPanelView::Join;
    }
    // Discovering is not a live session: switching tabs stays allowed.
    assert_eq!(app.document_switch_blocked(), None);

    assert!(app.activate_document_slot(0));

    assert_eq!(app.documents.active_id(), first);
    let collab = &app.host.editor_state().editor_ui.collab;
    assert_eq!(collab.phase, CollabConnectionPhase::Discovering);
    assert!(collab.panel.open);
}

#[test]
fn an_idle_panel_stays_with_its_tab_and_app_prefs_follow_the_user() {
    let mut app = app_with_work();
    let first = app.documents.active_id();
    {
        let collab = &mut app.host.editor_state_mut().editor_ui.collab;
        collab.availability = CollabAvailability::Ready;
        collab.panel.open = true;
    }
    assert!(app.open_untitled_tab());

    let collab = &app.host.editor_state().editor_ui.collab;
    assert!(
        !collab.panel.open,
        "the new tab does not show the old panel"
    );
    assert_eq!(collab.availability, CollabAvailability::Ready);
    parked_session(&mut app, first, |collab| assert!(collab.panel.open));
}

#[test]
fn the_tab_system_stays_frozen_during_a_live_session() {
    for phase in [
        CollabConnectionPhase::Starting,
        CollabConnectionPhase::Joining,
        CollabConnectionPhase::Authenticating,
        CollabConnectionPhase::Reconnecting,
        CollabConnectionPhase::ReadOnly,
        CollabConnectionPhase::Ended,
    ] {
        let mut app = app_with_work();
        app.host
            .editor_state_mut()
            .editor_ui
            .collab
            .set_phase(phase);
        assert_eq!(
            app.document_switch_blocked(),
            Some(crate::document_registry::BlockReason::Collaboration),
            "{phase:?}"
        );
        assert!(!app.open_untitled_tab(), "{phase:?}");
        assert_eq!(app.documents.len(), 1, "{phase:?}");
    }
}
