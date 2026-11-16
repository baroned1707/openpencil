//! The SESSION / APP classification of the collaboration display state.

use std::sync::Arc;

use super::super::*;
use crate::{CollabAdmissionRequestKey, CollabRelayRegion, PendingCollabAdmissionUi};

/// Every `CollabUiState` field that belongs to one document's session.
const SESSION_FIELDS: [&str; 14] = [
    "phase",
    "panel",
    "pending_edit",
    "notice",
    "discarded_edit",
    "pending_action",
    "authenticated",
    "public_session",
    "pending_admissions",
    "pending_owner_confirmation",
    "participants",
    "presence",
    "queued_presence",
    "last_presence_flush_ms",
];
/// Every `CollabUiState` field that belongs to the running app.
const APP_FIELDS: [&str; 2] = ["availability", "transport_capabilities"];
const PANEL_SESSION_FIELDS: [&str; 6] = [
    "open",
    "view",
    "join_input",
    "join_address_focused",
    "hover",
    "discovered",
];
const PANEL_APP_FIELDS: [&str; 1] = ["relay_region"];

/// Top-level field names of a pretty-printed struct (`{:#?}` indents a
/// field by exactly four spaces; nested values sit deeper).
fn debug_field_names(pretty: &str) -> Vec<String> {
    pretty
        .lines()
        .filter(|line| line.starts_with("    ") && !line.starts_with("     "))
        .filter_map(|line| {
            line.trim()
                .split_once(':')
                .map(|(name, _)| name.to_string())
        })
        .collect()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn listed(groups: &[&[&str]]) -> Vec<String> {
    sorted(
        groups
            .iter()
            .flat_map(|g| g.iter().map(|n| n.to_string()))
            .collect(),
    )
}

/// The destructure in `exchange_session` makes a new field fail to compile
/// until it is classified there; this keeps the human-readable lists above in
/// step with the real struct, so the classification can be read in one place.
#[test]
fn every_collab_ui_field_is_classified() {
    let state = format!("{:#?}", CollabUiState::default());
    assert_eq!(
        sorted(debug_field_names(&state)),
        listed(&[&SESSION_FIELDS, &APP_FIELDS]),
        "a CollabUiState field is missing from SESSION_FIELDS / APP_FIELDS"
    );
    let panel = format!("{:#?}", CollabPanelState::default());
    assert_eq!(
        sorted(debug_field_names(&panel)),
        listed(&[&PANEL_SESSION_FIELDS, &PANEL_APP_FIELDS]),
        "a CollabPanelState field is missing from the panel lists"
    );
}

fn participant(key: &str) -> CollabParticipantUi {
    CollabParticipantUi::new(key, "Ada Lovelace", 0x112233ff, CollabUiRole::Editor, false)
}

fn presence(key: &str, updated_at_ms: u64) -> RemotePresenceUi {
    RemotePresenceUi::bounded(key, None, ["n1".to_string()], None, updated_at_ms)
}

/// A session with every session field set to something other than its default.
pub(crate) fn populated_session() -> CollabUiState {
    let mut state = CollabUiState::default();
    assert!(state.set_authenticated_session(
        CollabConnectionPhase::Active,
        AuthenticatedCollabSession {
            session_name: "Quarterly plan".into(),
            role: CollabUiRole::Owner,
            share_endpoint: CollabShareEndpoint::new("10.0.0.5:4000"),
        },
        vec![participant("p1"), participant("p2")],
    ));
    state.set_public_session(
        crate::CollabInviteCode::new("ABCDEFGHJK"),
        crate::CollabConnectionPathUi::Relay {
            home_region: CollabRelayRegion::Global,
        },
    );
    state.queue_presence_snapshot(vec![presence("p1", 1)]);
    assert!(state.flush_presence(100));
    state.queue_presence_snapshot(vec![presence("p1", 2), presence("p2", 3)]);
    state.set_notice(CollabNoticeKind::OwnerLeft, 7);
    state.pending_edit = CollabPendingEditUi::Replaying;
    state.discarded_edit = Some(CollabDiscardedEditUi::bounded("Card", ["fill".to_string()]));
    state.pending_action = Some(CollabUiAction::JoinAddress {
        endpoint: "10.0.0.9:1".into(),
    });
    state.pending_admissions = Arc::new(vec![PendingCollabAdmissionUi::new(
        CollabAdmissionRequestKey::new("request-1").expect("valid key"),
        None,
    )]);
    state.pending_owner_confirmation = Some(crate::PendingOwnerConfirmationUi::new(
        CollabAdmissionRequestKey::new("request-2").expect("valid key"),
        crate::CollabOwnerIdentityUi::from_verified("owner-subject", "owner-device", None, None)
            .expect("verified identity"),
    ));
    state.panel.open = true;
    state.panel.view = CollabPanelView::Join;
    state.panel.join_input.set_text("10.0.0.9:1");
    state.panel.join_address_focused = true;
    state.panel.hover = Some(crate::CollabPanelHover::Close);
    state.panel.discovered = Arc::new(vec![DiscoveredCollabEndpoint {
        discovery_id: "d1".into(),
        endpoint: "10.0.0.9:1".into(),
        compatible: true,
    }]);
    state
}

/// Field-wise identity of the SESSION half: `Arc` payloads must be the same
/// allocation, not merely equal, so a swap provably moved the value instead of
/// rebuilding it. (`CollabUiState` has no `PartialEq`, and its `Debug` is
/// redacted.)
pub(crate) fn assert_session_identical(left: &CollabUiState, right: &CollabUiState) {
    let CollabUiState {
        availability: _,
        transport_capabilities: _,
        phase,
        panel,
        pending_edit,
        notice,
        discarded_edit,
        pending_action,
        authenticated,
        public_session,
        pending_admissions,
        pending_owner_confirmation,
        participants,
        presence,
        queued_presence,
        last_presence_flush_ms,
    } = left;
    assert_eq!(*phase, right.phase, "phase");
    assert_eq!(*pending_edit, right.pending_edit, "pending_edit");
    assert_eq!(*notice, right.notice, "notice");
    assert_eq!(*discarded_edit, right.discarded_edit, "discarded_edit");
    assert_eq!(*pending_action, right.pending_action, "pending_action");
    assert_eq!(*authenticated, right.authenticated, "authenticated");
    assert_eq!(*public_session, right.public_session, "public_session");
    assert!(
        Arc::ptr_eq(pending_admissions, &right.pending_admissions),
        "pending_admissions"
    );
    assert!(
        *pending_owner_confirmation == right.pending_owner_confirmation,
        "pending_owner_confirmation"
    );
    assert!(
        Arc::ptr_eq(participants, &right.participants),
        "participants"
    );
    assert!(Arc::ptr_eq(presence, &right.presence), "presence");
    assert!(*queued_presence == right.queued_presence, "queued_presence");
    assert_eq!(
        *last_presence_flush_ms, right.last_presence_flush_ms,
        "last_presence_flush_ms"
    );
    let CollabPanelState {
        open,
        view,
        join_input,
        join_address_focused,
        hover,
        discovered,
        relay_region: _,
    } = panel;
    assert_eq!(*open, right.panel.open, "panel.open");
    assert_eq!(*view, right.panel.view, "panel.view");
    assert!(*join_input == right.panel.join_input, "panel.join_input");
    assert_eq!(
        *join_address_focused, right.panel.join_address_focused,
        "panel.join_address_focused"
    );
    assert_eq!(*hover, right.panel.hover, "panel.hover");
    assert!(
        Arc::ptr_eq(discovered, &right.panel.discovered),
        "panel.discovered"
    );
}

fn with_app_fields(
    mut state: CollabUiState,
    availability: CollabAvailability,
    region: CollabRelayRegion,
) -> CollabUiState {
    state.availability = availability;
    state.transport_capabilities = crate::CollabTransportCapabilities::ALL;
    state.panel.relay_region = region;
    state
}

#[test]
fn exchange_session_moves_every_session_field_and_no_app_field() {
    let mut live = with_app_fields(
        CollabUiState::default(),
        CollabAvailability::Ready,
        CollabRelayRegion::Global,
    );
    let mut parked = populated_session();
    // `Clone` shares the `Arc` payloads, so these are exact references.
    let idle_before = live.clone();
    let populated_before = parked.clone();

    live.exchange_session(&mut parked);

    assert_session_identical(&live, &populated_before);
    assert_ne!(live.phase, CollabConnectionPhase::Idle);
    assert_session_identical(&parked, &idle_before);
    // Each side kept its OWN app fields.
    assert_eq!(live.availability, CollabAvailability::Ready);
    assert_eq!(
        live.transport_capabilities,
        crate::CollabTransportCapabilities::ALL
    );
    assert_eq!(live.panel.relay_region, CollabRelayRegion::Global);
    assert_eq!(parked.availability, CollabAvailability::Unavailable);
    assert_eq!(parked.panel.relay_region, CollabRelayRegion::default());
}

#[test]
fn take_session_leaves_an_idle_session_and_restore_puts_it_back() {
    let mut live = with_app_fields(
        populated_session(),
        CollabAvailability::Ready,
        CollabRelayRegion::Global,
    );
    let before = live.clone();

    let taken = live.take_session();
    assert_eq!(live.phase, CollabConnectionPhase::Idle);
    assert!(live.pending_action.is_none());
    assert!(!live.panel.open);
    assert!(live.participants().is_empty());
    assert_eq!(
        live.availability,
        CollabAvailability::Ready,
        "app field stays"
    );

    live.restore_session(taken);
    assert_session_identical(&live, &before);
}
