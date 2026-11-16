//! The life of one UI-thread request, shared by the connection thread that
//! sent it and the UI pump that will (or will not) run it.
//!
//! The connection thread gives up after its budget. Without a shared record the
//! request would still sit in the queue and be applied whenever the UI becomes
//! responsive, so a write the agent was told had failed would land anyway and a
//! retry would double it. The ticket closes that gap: exactly one side decides.
//!
//! ```text
//!   Pending --pump starts it--> Started --pump answers--> Done
//!      \--caller times out, or the pump finds the deadline passed--> Cancelled
//! ```
//!
//! Every transition out of `Pending` is a compare-and-swap, so whichever side
//! gets there first wins and the loser sees it. A `Cancelled` request is never
//! applied. A `Started` request always completes, and its caller waits a short
//! grace for the answer instead of claiming a failure that may not be true.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

const PENDING: u8 = 0;
const STARTED: u8 = 1;
const DONE: u8 = 2;
const CANCELLED: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TicketState {
    Pending,
    Started,
    Done,
    Cancelled,
}

/// What the caller's attempt to cancel found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cancel {
    /// The caller won: the request will never run.
    Won,
    /// The pump (or an earlier cancel) already dropped it: it never ran.
    AlreadyCancelled,
    /// The pump started it first: it runs to completion and will answer.
    TooLate,
}

#[derive(Debug)]
pub(super) struct Ticket {
    state: AtomicU8,
    /// The caller's own wait budget, as an absolute time on the server's clock.
    deadline: Instant,
}

impl Ticket {
    pub(super) fn new(deadline: Instant) -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU8::new(PENDING),
            deadline,
        })
    }

    /// A ticket that never expires, for tests that feed the pump by hand.
    #[cfg(test)]
    pub(super) fn unbounded() -> Arc<Self> {
        Self::new(Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600))
    }

    pub(super) fn deadline(&self) -> Instant {
        self.deadline
    }

    pub(super) fn state(&self) -> TicketState {
        match self.state.load(Ordering::SeqCst) {
            PENDING => TicketState::Pending,
            STARTED => TicketState::Started,
            DONE => TicketState::Done,
            _ => TicketState::Cancelled,
        }
    }

    /// Pump side: may the request run? `false` means drop it unapplied, either
    /// because its deadline has passed (it is cancelled here, so the caller
    /// cannot see it as started) or because the caller already cancelled it.
    pub(super) fn try_start(&self, now: Instant) -> bool {
        if now >= self.deadline {
            let _ = self.swap(PENDING, CANCELLED);
            return false;
        }
        self.swap(PENDING, STARTED)
    }

    /// Pump side: the started request has been answered.
    pub(super) fn finish(&self) {
        let _ = self.swap(STARTED, DONE);
    }

    /// Caller side: stop the request if the pump has not started it.
    pub(super) fn cancel(&self) -> Cancel {
        match self
            .state
            .compare_exchange(PENDING, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => Cancel::Won,
            Err(CANCELLED) => Cancel::AlreadyCancelled,
            Err(_) => Cancel::TooLate,
        }
    }

    fn swap(&self, from: u8, to: u8) -> bool {
        self.state
            .compare_exchange(from, to, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

/// Cancels a still-pending request when the caller goes away for ANY reason,
/// including a panic unwinding through the connection thread, so a request an
/// unwinding caller queued can never apply later.
pub(super) struct CancelOnDrop(Arc<Ticket>);

impl CancelOnDrop {
    pub(super) fn new(ticket: &Arc<Ticket>) -> Self {
        Self(Arc::clone(ticket))
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = self.0.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn exactly_one_side_wins_the_race_out_of_pending() {
        let now = Instant::now();
        let started_first = Ticket::new(now + Duration::from_secs(5));
        assert!(started_first.try_start(now));
        assert_eq!(started_first.cancel(), Cancel::TooLate);
        assert_eq!(started_first.state(), TicketState::Started);

        let cancelled_first = Ticket::new(now + Duration::from_secs(5));
        assert_eq!(cancelled_first.cancel(), Cancel::Won);
        assert!(
            !cancelled_first.try_start(now),
            "a cancelled request never starts"
        );
        assert_eq!(cancelled_first.cancel(), Cancel::AlreadyCancelled);
    }

    #[test]
    fn a_pump_that_finds_the_deadline_passed_cancels_the_request() {
        let now = Instant::now();
        let ticket = Ticket::new(now);
        assert!(!ticket.try_start(now));
        assert_eq!(ticket.state(), TicketState::Cancelled);
        assert_eq!(ticket.cancel(), Cancel::AlreadyCancelled);
    }

    #[test]
    fn a_panicking_caller_cancels_its_pending_request() {
        let ticket = Ticket::new(Instant::now() + Duration::from_secs(5));
        let for_thread = Arc::clone(&ticket);
        let unwound = std::thread::spawn(move || {
            let _guard = CancelOnDrop::new(&for_thread);
            panic!("connection thread dies mid-call");
        })
        .join();
        assert!(unwound.is_err());
        assert!(
            !ticket.try_start(Instant::now()),
            "the queued request is dead"
        );
    }

    #[test]
    fn the_guard_leaves_a_started_request_alone() {
        let ticket = Ticket::new(Instant::now() + Duration::from_secs(5));
        assert!(ticket.try_start(Instant::now()));
        drop(CancelOnDrop::new(&ticket));
        assert_eq!(ticket.state(), TicketState::Started);
    }
}
