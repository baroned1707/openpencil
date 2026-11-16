//! The clock and wait budgets the live server's two halves agree on, and the
//! request channel that carries them to the connection threads.
//!
//! A request's deadline is computed by the connection thread from this clock
//! and compared by the UI pump against the SAME clock, so a test can move time
//! with one handle instead of sleeping through a 15 s budget.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SendError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::UiRequest;

/// How long a caller keeps waiting for the answer of a request the UI thread
/// has already started, after its own budget ran out. Bounded so a UI thread
/// stuck INSIDE an apply cannot hold the connection forever.
pub(super) const STARTED_GRACE: Duration = Duration::from_secs(2);

/// Source of "now". A plain function so a test can supply its own time.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone)]
pub(super) struct LiveTiming {
    clock: Arc<dyn Clock>,
    /// How long a caller waits for the UI thread to start a request.
    pub(super) ack_budget: Duration,
    /// How long it keeps waiting once the UI thread has started one.
    pub(super) grace: Duration,
    /// Times a caller found its request already started and waited the grace.
    grace_waits: Arc<AtomicUsize>,
}

impl LiveTiming {
    pub(super) fn new(clock: Arc<dyn Clock>, ack_budget: Duration, grace: Duration) -> Self {
        Self {
            clock,
            ack_budget,
            grace,
            grace_waits: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(super) fn system() -> Self {
        Self::new(Arc::new(SystemClock), super::UI_ACK_TIMEOUT, STARTED_GRACE)
    }

    pub(super) fn with_clock(clock: Arc<dyn Clock>) -> Self {
        Self::new(clock, super::UI_ACK_TIMEOUT, STARTED_GRACE)
    }

    pub(super) fn now(&self) -> Instant {
        self.clock.now()
    }

    /// The absolute deadline of a request sent now.
    pub(super) fn deadline(&self) -> Instant {
        self.now() + self.ack_budget
    }

    pub(super) fn note_grace_wait(&self) {
        self.grace_waits.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(super) fn grace_waits(&self) -> usize {
        self.grace_waits.load(Ordering::SeqCst)
    }
}

/// The connection side of the request queue: the channel to the UI pump plus
/// the timing every request on it is held to.
#[derive(Clone)]
pub(super) struct UiSender {
    tx: Sender<UiRequest>,
    timing: LiveTiming,
}

impl UiSender {
    pub(super) fn new(tx: Sender<UiRequest>, timing: LiveTiming) -> Self {
        Self { tx, timing }
    }

    /// `Err(())` when the pump is gone (the editor is shutting down).
    pub(super) fn send(&self, request: UiRequest) -> Result<(), ()> {
        self.tx.send(request).map_err(|_: SendError<UiRequest>| ())
    }

    pub(super) fn timing(&self) -> &LiveTiming {
        &self.timing
    }
}

/// A queue on the real clock.
impl From<Sender<UiRequest>> for UiSender {
    fn from(tx: Sender<UiRequest>) -> Self {
        Self::new(tx, LiveTiming::system())
    }
}

/// A new queue: the connection side and the pump's receiving end.
#[cfg(test)]
pub(super) fn channel(timing: LiveTiming) -> (UiSender, std::sync::mpsc::Receiver<UiRequest>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (UiSender::new(tx, timing), rx)
}
