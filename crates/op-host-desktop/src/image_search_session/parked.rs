//! What the frame loop asks of the image session of a PARKED document.
//!
//! The session of the document on screen is driven every frame against the
//! host's current scene. A parked document has no current scene, so its
//! session is walked scene-less, at most once per [`PARKED_SCAN_GAP`], and only
//! when its content or active page moved since the last walk; polling finished
//! jobs is a cheap channel read and happens every frame while anything is in
//! flight. The test seam at the bottom stands in for the network.

use super::*;
use std::time::Duration;

/// A parked document's tree is walked at most this often. An agent that keeps
/// writing into a parked document moves its revision every few milliseconds;
/// each walk resolves a whole page, so it must not run on every frame.
pub(crate) const PARKED_SCAN_GAP: Duration = Duration::from_millis(250);

/// No new parked scan starts while this many jobs are in flight across all
/// documents, so a handful of open documents cannot multiply the number of
/// concurrent provider ladders.
pub(crate) const PARKED_PENDING_CAP: usize = 16;

/// What one parked document needs this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParkedImageWork {
    /// Jobs are in flight: read their results.
    pub poll: bool,
    /// The document changed since its last walk and the gap has passed: walk
    /// it for empty image slots.
    pub scan: bool,
}

impl ParkedImageWork {
    /// Nothing to do: the frame need not enter the document at all.
    pub(crate) fn is_idle(self) -> bool {
        !self.poll && !self.scan
    }
}

impl ImageSearchSession {
    /// Whether a parked document with this `revision` and active `page` holds
    /// content nobody has looked at for empty image slots yet. A document that
    /// was never edited this session (revision 0) is exactly what the file
    /// holds, so it waits for the user to open it, as it always did.
    pub(crate) fn parked_scan_waiting(&self, revision: u64, page: usize) -> bool {
        revision > 0 && self.last_scanned != Some((revision, page))
    }

    /// The work a parked document with this `revision` and active `page`
    /// needs at `now`. Pure, so the budget rules are testable without a window.
    pub(crate) fn parked_work(&self, revision: u64, page: usize, now: Instant) -> ParkedImageWork {
        let moved = self.parked_scan_waiting(revision, page);
        let gap_passed = self
            .last_parked_scan
            .is_none_or(|at| now.saturating_duration_since(at) >= PARKED_SCAN_GAP);
        ParkedImageWork {
            poll: self.is_pending(),
            scan: moved && gap_passed,
        }
    }

    /// Record that a parked walk happened at `now`.
    pub(crate) fn note_parked_scan(&mut self, now: Instant) {
        self.last_parked_scan = Some(now);
    }

    /// Number of jobs in flight.
    pub(crate) fn pending_jobs(&self) -> usize {
        self.jobs.len()
    }
}

/// What a stubbed search finds for a slot's query.
#[cfg(test)]
pub(crate) type StubbedFinder = fn(&str) -> Option<FoundImage>;

/// How the test stand-in for the network answers a stock search.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) enum StubbedSearch {
    /// Answer at once.
    Answer(StubbedFinder),
    /// Keep every search in flight until [`release_held_searches`].
    Hold,
}

#[cfg(test)]
thread_local! {
    static STUBBED_SEARCH: std::cell::RefCell<Option<StubbedSearch>> =
        const { std::cell::RefCell::new(None) };
    /// `(slot query, answer channel)` of every search held in flight.
    static HELD_SEARCHES: std::cell::RefCell<Vec<(String, mpsc::Sender<JobOutcome>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Make every stock search started on this thread answer from the stub
/// instead of the network (`None` restores the real ladder and drops any held
/// search).
#[cfg(test)]
pub(crate) fn stub_searches(stub: Option<StubbedSearch>) {
    if stub.is_none() {
        HELD_SEARCHES.with(|held| held.borrow_mut().clear());
    }
    STUBBED_SEARCH.with(|cell| *cell.borrow_mut() = stub);
}

/// Answer every held search with `finder(query)` and forget them. Returns how
/// many searches were still waiting for an answer.
#[cfg(test)]
pub(crate) fn release_held_searches(finder: StubbedFinder) -> usize {
    let held = HELD_SEARCHES.with(|held| std::mem::take(&mut *held.borrow_mut()));
    let count = held.len();
    for (query, sender) in held {
        let _ = sender.send(JobOutcome::Search(finder(&query)));
    }
    count
}

/// The job a stubbed search becomes; `None` when no stub is set.
#[cfg(test)]
pub(super) fn stubbed_search_job(
    target: &ImageSearchTarget,
    intent: &str,
    deadline: Option<Instant>,
) -> Option<ImageSearchJob> {
    let stub = STUBBED_SEARCH.with(|cell| *cell.borrow())?;
    let (tx, rx) = mpsc::channel();
    match stub {
        StubbedSearch::Answer(finder) => {
            let _ = tx.send(JobOutcome::Search(finder(&target.query)));
        }
        StubbedSearch::Hold => HELD_SEARCHES.with(|held| {
            held.borrow_mut().push((target.query.clone(), tx));
        }),
    }
    Some(ImageSearchJob {
        node_id: target.node_id.clone(),
        deadline,
        intent: Some(intent.to_string()),
        stock_view: Some(target.judge_view),
        fallback_target: None,
        rx,
    })
}
