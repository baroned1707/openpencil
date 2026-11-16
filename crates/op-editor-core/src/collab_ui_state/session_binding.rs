//! Whether a collaboration session is bound to a document, and in which
//! capacity.
//!
//! Hosts that keep several documents open decide per document what may still
//! touch it (tab switching, recovery snapshots, closing). They all ask the same
//! questions, so the answers live next to the state they read instead of being
//! re-derived from the phase at every call site.

use super::{CollabConnectionPhase, CollabUiRole, CollabUiState};

impl CollabConnectionPhase {
    /// A session owns the document: every phase except `Idle` and
    /// `Discovering`. Discovery lists endpoints without binding or replacing
    /// the document, so it does not count.
    pub const fn binds_session(self) -> bool {
        !matches!(self, Self::Idle | Self::Discovering)
    }

    /// A session that is not established yet: the transport is still being
    /// brought up or admission is still running. Nothing can be switched away
    /// from it, because the document it is about to install into would be
    /// left half-bound.
    pub const fn is_being_established(self) -> bool {
        matches!(self, Self::Starting | Self::Joining | Self::Authenticating)
    }
}

impl CollabUiState {
    /// See [`CollabConnectionPhase::binds_session`].
    pub const fn binds_session(&self) -> bool {
        self.phase.binds_session()
    }

    /// Everyone in the session except this peer. This is how many people an
    /// owner ends the session for by closing it.
    pub fn other_participant_count(&self) -> usize {
        self.participants()
            .iter()
            .filter(|participant| !participant.is_self)
            .count()
    }

    /// Whether the session lets this peer own the document.
    pub fn is_session_owner(&self) -> bool {
        self.authenticated_session()
            .is_some_and(|session| session.role == CollabUiRole::Owner)
            || self.phase == CollabConnectionPhase::Starting
    }

    /// Whether the document is another person's shared document, held as a
    /// guest (also while the join is still being admitted, and after the
    /// session ended and left the guest with an independent copy). The owner's
    /// own document, and any document without a session, are not.
    pub fn holds_a_guests_shared_document(&self) -> bool {
        self.binds_session() && !self.is_session_owner()
    }

    /// Whether the session can no longer carry on by itself and the user has
    /// to look: it ended (the owner left, the epoch changed), or the link is
    /// down and the last attempt reported a failure. A tab that is not on
    /// screen cannot show the panel notice, so hosts mark it from this.
    pub fn session_lost(&self) -> bool {
        match self.phase {
            CollabConnectionPhase::Ended => true,
            CollabConnectionPhase::Reconnecting => self.notice.is_some(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::AuthenticatedCollabSession;
    use super::*;

    fn session(phase: CollabConnectionPhase, role: CollabUiRole) -> CollabUiState {
        let mut state = CollabUiState::default();
        state.set_authenticated_session(
            phase,
            AuthenticatedCollabSession {
                session_name: "review".into(),
                role,
                share_endpoint: None,
            },
            Vec::new(),
        );
        state
    }

    #[test]
    fn discovery_and_idle_bind_nothing() {
        assert!(!CollabConnectionPhase::Idle.binds_session());
        assert!(!CollabConnectionPhase::Discovering.binds_session());
        for phase in [
            CollabConnectionPhase::Starting,
            CollabConnectionPhase::Joining,
            CollabConnectionPhase::Authenticating,
            CollabConnectionPhase::Active,
            CollabConnectionPhase::Reconnecting,
            CollabConnectionPhase::ReadOnly,
            CollabConnectionPhase::Ended,
        ] {
            assert!(phase.binds_session(), "{phase:?}");
        }
    }

    #[test]
    fn only_the_start_up_phases_are_still_being_established() {
        for phase in [
            CollabConnectionPhase::Starting,
            CollabConnectionPhase::Joining,
            CollabConnectionPhase::Authenticating,
        ] {
            assert!(phase.is_being_established(), "{phase:?}");
        }
        for phase in [
            CollabConnectionPhase::Idle,
            CollabConnectionPhase::Discovering,
            CollabConnectionPhase::Active,
            CollabConnectionPhase::Reconnecting,
            CollabConnectionPhase::ReadOnly,
            CollabConnectionPhase::Ended,
        ] {
            assert!(!phase.is_being_established(), "{phase:?}");
        }
    }

    #[test]
    fn a_guest_holds_a_shared_document_and_an_owner_does_not() {
        assert!(!CollabUiState::default().holds_a_guests_shared_document());
        let owner = session(CollabConnectionPhase::Active, CollabUiRole::Owner);
        assert!(!owner.holds_a_guests_shared_document());
        for role in [CollabUiRole::Editor, CollabUiRole::Viewer] {
            for phase in [
                CollabConnectionPhase::Active,
                CollabConnectionPhase::Reconnecting,
                CollabConnectionPhase::Ended,
            ] {
                assert!(
                    session(phase, role).holds_a_guests_shared_document(),
                    "{role:?} {phase:?}"
                );
            }
        }
    }

    #[test]
    fn a_session_is_lost_when_it_ended_or_its_link_failed() {
        let notice = super::super::CollabNoticeKind::DisconnectedReadOnly;
        let mut state = session(CollabConnectionPhase::Active, CollabUiRole::Editor);
        assert!(!state.session_lost());
        state.set_phase(CollabConnectionPhase::Reconnecting);
        assert!(!state.session_lost(), "reconnecting without a failure");
        state.set_notice(notice, 1);
        assert!(state.session_lost(), "reconnecting after a failure");
        state.set_phase(CollabConnectionPhase::Ended);
        assert!(state.session_lost());
        state.set_phase(CollabConnectionPhase::Active);
        assert!(!state.session_lost(), "reconnected");
        assert!(!CollabUiState::default().session_lost());
    }

    #[test]
    fn a_starting_owner_and_a_joining_guest_are_told_apart_without_a_role() {
        let mut starting = CollabUiState::default();
        starting.set_phase(CollabConnectionPhase::Starting);
        assert!(!starting.holds_a_guests_shared_document());
        for phase in [
            CollabConnectionPhase::Joining,
            CollabConnectionPhase::Authenticating,
        ] {
            let mut joining = CollabUiState::default();
            joining.set_phase(phase);
            assert!(joining.holds_a_guests_shared_document(), "{phase:?}");
        }
    }
}
