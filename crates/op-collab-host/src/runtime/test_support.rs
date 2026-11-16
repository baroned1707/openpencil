//! Controlled collaboration fixtures for downstream test suites.
//!
//! `op-host-services` needs to drive the REAL begin → install → reject/fail
//! paths to prove what its ingest does with a session's verdict. Without a
//! fixture its tests can only assert against a hand-built `LocalEditOutcome`,
//! which passes whether or not the wiring behind it is correct.
//!
//! Standing up an activated owner session needs `crate::runtime`'s private
//! actor and channel internals, so this module lives inside it and re-exports
//! exactly two capabilities:
//!
//! 1. [`owner_session`] — an activated owner runtime over a caller-supplied
//!    baseline document, ready for `begin_local_edit`.
//! 2. [`owner_session_with_saturated_command_lane`] — the same, with the
//!    outbound command lane already full, so the next commit cannot be
//!    delivered and the runtime falls back to standalone.
//! 3. [`guest_session_with_pending_remote_commit`], [`queue_ticket_renewal`]
//!    and [`queue_transport_loss`] — what the network worker would deliver to
//!    the GUI thread, queued for the runtime's next `poll`. A host that polls
//!    runtimes it does not draw (a parked document) proves its wiring with
//!    these; the protocol itself is covered by this crate's own tests.
//!
//! ## The lane guard is not optional
//!
//! Both constructors return an [`OwnerLaneGuard`] alongside the runtime, and
//! the caller MUST hold it for as long as it drives the runtime. The guard
//! owns the channel's receiver, and a bounded `SyncSender` reports two
//! different failures: `Full` when the lane is saturated, `Disconnected` once
//! the receiver is gone (`network.rs`'s `NetworkCommandSendError`, which the
//! runtime maps to `ResourceLimit` and `Transport` respectively). Drop the
//! guard early and the "full lane" test silently becomes a "dead channel"
//! test — a different code path with a different projected failure.
//!
//! ## Not a production surface
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]`, and the feature
//! is turned on only by a `dev-dependencies` entry. A normal build of this
//! crate compiles none of it, so it cannot reach the shipped API.

use std::sync::mpsc::Receiver;

use op_collab::{
    CollabMessage, ConnectionKey, Epoch, FrameEnvelope, OpaqueTicket, Role, SessionId,
    VerifiedAuthMetadata,
};
use op_editor_core::PenDocument;

use super::actor::{set_guest_ui, set_owner_ui, EditorActor, GuestActor, OwnerActor};
use super::network::{
    guest_command_channel_with_capacity_for_test, owner_command_channel_with_capacity_for_test,
};
use super::types::{
    CollabRuntimeFailure, GuestNetworkCommand, NetworkEvent, OwnerNetworkCommand,
    TerminalNetworkEvent,
};
use super::CollabRuntime;
use crate::host::HeadlessCollabHost;

/// Session id every fixture runs under.
const FIXTURE_SESSION: &str = "collab-host-test-support";

/// Room for exactly one command, which the saturating constructor then uses.
const FIXTURE_COMMAND_CAPACITY: usize = 1;

/// Keeps the owner's outbound command lane connected.
///
/// Hold it for the whole test — see the module docs for why dropping it early
/// silently changes which failure the runtime reports.
#[must_use = "dropping the guard disconnects the lane and changes the failure \
              the runtime reports from Full to Disconnected"]
pub struct OwnerLaneGuard {
    _commands: Receiver<OwnerNetworkCommand>,
}

impl OwnerLaneGuard {
    /// Drain and count commands emitted since the previous observation.
    ///
    /// This exists only for downstream runtime-wiring tests that must prove a
    /// UI gesture closes into exactly one collaboration commit.
    pub fn drain_command_count(&self) -> usize {
        self._commands.try_iter().count()
    }
}

/// Build an owner runtime with one admitted editor peer, ready for
/// `begin_local_edit`.
///
/// `baseline` is the document the session is activated over, and it must be
/// the same document the caller's own editor holds. The owner core validates
/// each commit against the document the capture opened on, so a session
/// activated over a *different* document reports a candidate mismatch instead
/// of the delivery outcome the caller is trying to observe.
pub fn owner_session(baseline: PenDocument) -> (CollabRuntime, OwnerLaneGuard) {
    build(baseline, false)
}

/// [`owner_session`] with the outbound command lane already full.
///
/// The next commit cannot be handed to the network worker, so the runtime
/// retires the session and `finish_local_edit` reports
/// `Failed { document_rolled_back: false }` — the standalone fallback, which
/// deliberately KEEPS the edit because the user's work is still theirs even
/// though the session is gone. The projected failure is `ResourceLimit`, which
/// is how a caller can tell this apart from a dead channel.
pub fn owner_session_with_saturated_command_lane(
    baseline: PenDocument,
) -> (CollabRuntime, OwnerLaneGuard) {
    build(baseline, true)
}

/// Keeps a guest's outbound command lane connected; see [`OwnerLaneGuard`].
#[must_use = "dropping the guard disconnects the lane the guest answers on"]
pub struct GuestLaneGuard {
    _commands: Receiver<GuestNetworkCommand>,
}

impl GuestLaneGuard {
    /// Drain and count commands the guest emitted since the last observation.
    pub fn drain_command_count(&self) -> usize {
        self._commands.try_iter().count()
    }
}

/// The connection every guest fixture runs on.
fn guest_fixture_connection() -> ConnectionKey {
    ConnectionKey::new(2).expect("non-zero connection")
}

/// An established guest session over `baseline` whose owner has already
/// committed `remote`: the commit sits in the runtime's event lane, so the next
/// `poll` applies it to whichever host it is handed. That host's document must
/// equal `baseline` for the commit to apply.
pub fn guest_session_with_pending_remote_commit(
    baseline: PenDocument,
    remote: PenDocument,
) -> (CollabRuntime, GuestLaneGuard) {
    let connection = guest_fixture_connection();
    let mut owner_host = HeadlessCollabHost::new();
    owner_host.editor_state_mut().doc = baseline;
    let mut owner = OwnerActor::new(
        SessionId::from(FIXTURE_SESSION),
        Epoch(1),
        fixture_auth(0),
        &mut owner_host,
    )
    .expect("owner actor");
    let grant = owner
        .grant_new_peer(fixture_auth(1), Role::Editor)
        .expect("peer grant");
    let activation = owner
        .session
        .activate_peer(connection, grant, &owner_host)
        .expect("activate peer");

    // The guest's own host is a throwaway: the actor owns its session state.
    let mut guest_host = HeadlessCollabHost::new();
    let mut guest = GuestActor::new(
        SessionId::from(FIXTURE_SESSION),
        Epoch(1),
        activation.welcome,
        connection,
        &mut guest_host,
    )
    .expect("guest actor");
    guest
        .session
        .accept_frame(
            FrameEnvelope::new(
                SessionId::from(FIXTURE_SESSION),
                Epoch(1),
                CollabMessage::Snapshot(Box::new(activation.snapshot.expect("snapshot"))),
            ),
            &mut guest_host,
        )
        .expect("guest installs the snapshot");
    set_guest_ui(
        &mut guest_host,
        &guest,
        op_editor_core::CollabConnectionPhase::Active,
    );

    owner
        .session
        .begin_local_edit(&owner_host)
        .expect("owner begins an edit");
    owner_host.editor_state_mut().doc = remote;
    let output = owner
        .session
        .finish_local_edit(&mut owner_host)
        .expect("owner commits");
    let commit = output
        .effects
        .iter()
        .find_map(|effect| match effect {
            op_collab::OwnerEffect::BroadcastCommit { commit } => Some(commit.as_ref().clone()),
            _ => None,
        })
        .expect("an owner edit broadcasts a commit");

    let (network, commands) = guest_command_channel_with_capacity_for_test(8);
    let mut runtime = CollabRuntime::new();
    runtime.network = Some(network);
    runtime.actor = Some(EditorActor::Guest(Box::new(guest)));
    // A joined guest is pinned to its owner, which is what lets it retry on its
    // own after a dropped link.
    runtime.last_join = Some(super::relay::GuestConnectionRoute::lan(
        vec!["127.0.0.1:43120".parse().expect("loopback address")],
        None,
        None,
    ));
    runtime.pinned_owner_static = Some([0x5a; 32]);
    runtime
        .event_sink()
        .try_send(NetworkEvent::Frame {
            connection,
            frame: FrameEnvelope::new(
                SessionId::from(FIXTURE_SESSION),
                Epoch(1),
                CollabMessage::Commit(commit),
            ),
        })
        .expect("room for one event");
    (
        runtime,
        GuestLaneGuard {
            _commands: commands,
        },
    )
}

/// Queue what the ticket-renewal worker delivers when a fresh credential is
/// ready: the runtime forwards it to its peers on the next `poll`.
pub fn queue_ticket_renewal(runtime: &CollabRuntime) {
    let ticket = OpaqueTicket::new("fixture-renewed-ticket".to_string()).expect("non-empty");
    runtime
        .event_sink()
        .try_send(NetworkEvent::LocalTicketReady { ticket })
        .expect("room for one event");
}

/// Queue the end of the guest fixture's connection, as its worker reports a
/// dropped link: the next `poll` moves the session to `Reconnecting`.
pub fn queue_transport_loss(runtime: &CollabRuntime) {
    assert!(runtime
        .event_sink()
        .send_terminal(TerminalNetworkEvent::ConnectionClosed {
            connection: guest_fixture_connection(),
            failure: Some(CollabRuntimeFailure::Transport),
            remote_bye: None,
        }));
}

fn build(baseline: PenDocument, saturate: bool) -> (CollabRuntime, OwnerLaneGuard) {
    let mut host = HeadlessCollabHost::new();
    host.editor_state_mut().doc = baseline;
    let mut owner = OwnerActor::new(
        SessionId::from(FIXTURE_SESSION),
        Epoch(1),
        fixture_auth(0),
        &mut host,
    )
    .expect("owner actor");
    let peer = ConnectionKey::new(2).expect("non-zero connection");
    let grant = owner
        .grant_new_peer(fixture_auth(1), Role::Editor)
        .expect("peer grant");
    owner
        .session
        .activate_peer(peer, grant, &host)
        .expect("activate peer");
    owner.connections.insert(peer);
    set_owner_ui(&mut host, &owner);

    let (network, commands) =
        owner_command_channel_with_capacity_for_test(FIXTURE_COMMAND_CAPACITY);
    let mut runtime = CollabRuntime::new();
    runtime.network = Some(network);
    runtime.actor = Some(EditorActor::Owner(Box::new(owner)));
    if saturate {
        runtime
            .send_owner(OwnerNetworkCommand::Close {
                connection: ConnectionKey::new(99).expect("non-zero connection"),
            })
            .expect("the first send fills the lane");
    }
    // `host` is dropped here on purpose: the actor owns its own session state,
    // and this host existed only to seed the baseline and take the projection.
    (
        runtime,
        OwnerLaneGuard {
            _commands: commands,
        },
    )
}

fn fixture_auth(index: usize) -> VerifiedAuthMetadata {
    VerifiedAuthMetadata {
        issuer: "https://issuer.example".into(),
        subject: format!("subject-{index}"),
        device_id: format!("device-{index}"),
        proof_binding: format!("binding-{index}"),
        expires_at_unix_ms: 10_000,
        display_name: None,
        avatar_url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::types::CollabRuntimeFailure;

    /// The bug this module's guard exists to prevent: a caller that lets the
    /// receiver drop gets `Disconnected` (projected as `Transport`) where it
    /// meant to test `Full` (projected as `ResourceLimit`).
    #[test]
    fn a_saturated_lane_reports_resource_limit_while_the_guard_is_held() {
        let (runtime, guard) =
            owner_session_with_saturated_command_lane(op_editor_core::EditorState::new().doc);

        let error = runtime
            .send_owner(OwnerNetworkCommand::Close {
                connection: ConnectionKey::new(98).expect("non-zero connection"),
            })
            .expect_err("the lane is already full");
        assert_eq!(
            error.failure,
            CollabRuntimeFailure::ResourceLimit,
            "a full lane is a resource limit, not a transport failure"
        );
        drop(guard);
    }

    #[test]
    fn dropping_the_guard_turns_the_same_send_into_a_transport_failure() {
        // Pins the distinction the guard protects, so a future refactor that
        // silently drops the receiver fails here rather than in a downstream
        // test that looks like it is passing.
        let (runtime, guard) =
            owner_session_with_saturated_command_lane(op_editor_core::EditorState::new().doc);
        drop(guard);

        let error = runtime
            .send_owner(OwnerNetworkCommand::Close {
                connection: ConnectionKey::new(98).expect("non-zero connection"),
            })
            .expect_err("the receiver is gone");
        assert_eq!(error.failure, CollabRuntimeFailure::Transport);
    }

    fn rectangle_named(name: &str) -> PenDocument {
        serde_json::from_value(serde_json::json!({
            "version": "1.0",
            "children": [{
                "type": "rectangle", "id": "shared-node", "name": name,
                "x": 0, "y": 0, "width": 20, "height": 20
            }]
        }))
        .unwrap()
    }

    #[test]
    fn a_pending_remote_commit_applies_on_the_next_poll() {
        let (mut runtime, _guard) = guest_session_with_pending_remote_commit(
            rectangle_named("Before"),
            rectangle_named("Remote"),
        );
        let mut host = HeadlessCollabHost::new();
        host.editor_state_mut().doc = rectangle_named("Before");
        host.editor_state_mut().editor_ui.collab.phase =
            op_editor_core::CollabConnectionPhase::Active;

        runtime.poll(&mut host);

        assert_eq!(
            op_collab::canonical_document_hash(&host.editor_state().doc).unwrap(),
            op_collab::canonical_document_hash(&rectangle_named("Remote")).unwrap(),
        );
    }

    #[test]
    fn a_queued_transport_loss_schedules_a_reconnect_deadline() {
        let (mut runtime, _guard) = guest_session_with_pending_remote_commit(
            rectangle_named("Before"),
            rectangle_named("Remote"),
        );
        let mut host = HeadlessCollabHost::new();
        host.editor_state_mut().doc = rectangle_named("Before");
        runtime.poll(&mut host);
        assert!(runtime.next_reconnect_deadline().is_none());
        queue_transport_loss(&runtime);

        runtime.poll(&mut host);

        assert!(runtime.next_reconnect_deadline().is_some());
    }

    #[test]
    fn a_queued_transport_loss_moves_the_session_to_reconnecting() {
        let (mut runtime, _guard) = guest_session_with_pending_remote_commit(
            rectangle_named("Before"),
            rectangle_named("Remote"),
        );
        let mut host = HeadlessCollabHost::new();
        host.editor_state_mut().doc = rectangle_named("Before");
        runtime.poll(&mut host);
        queue_transport_loss(&runtime);

        runtime.poll(&mut host);

        assert_eq!(
            host.editor_state().editor_ui.collab.phase,
            op_editor_core::CollabConnectionPhase::Reconnecting
        );
    }

    #[test]
    fn a_queued_ticket_renewal_is_forwarded_to_the_peers() {
        let (mut runtime, guard) = owner_session(rectangle_named("Doc"));
        let mut host = HeadlessCollabHost::new();
        host.editor_state_mut().doc = rectangle_named("Doc");
        assert_eq!(guard.drain_command_count(), 0);
        queue_ticket_renewal(&runtime);

        runtime.poll(&mut host);

        assert!(
            guard.drain_command_count() >= 1,
            "a renewal reached the peer lane"
        );
    }

    #[test]
    fn an_unsaturated_lane_accepts_one_command() {
        let (runtime, _guard) = owner_session(op_editor_core::EditorState::new().doc);
        assert!(runtime
            .send_owner(OwnerNetworkCommand::Close {
                connection: ConnectionKey::new(98).expect("non-zero connection"),
            })
            .is_ok());
    }
}
