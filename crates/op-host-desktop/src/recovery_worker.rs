//! The thread that writes recovery snapshots.
//!
//! The UI thread captures a snapshot (a structurally shared copy, the same
//! capture a background Save takes) and hands it over; this thread serializes
//! it and does the file IO, so a large document never stalls a frame. Jobs run
//! in order on one thread. Ordering between snapshots of the same document is
//! guarded by the store's tickets, not by the queue, so the exit path may
//! submit a newer snapshot while an older one is still being written.
//!
//! The thread starts with the first job. A panic while writing is caught and
//! reported as a failed write; the thread keeps serving.

use crate::recovery_error::RecoveryError;
use crate::recovery_store::{RecoveryStore, SnapshotInfo, Ticket, WriteOutcome};
use op_host_services::doc_io::CanonicalSaveSnapshot;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// One snapshot to write.
pub(crate) struct WriteJob {
    pub document: u64,
    /// `(generation, revision)` of the document when it was captured.
    pub revision: (u64, u64),
    pub ticket: Ticket,
    pub snapshot: Box<CanonicalSaveSnapshot>,
    pub info: SnapshotInfo,
}

/// What a job came to.
pub(crate) struct WriteResult {
    pub document: u64,
    pub revision: (u64, u64),
    pub ticket: Ticket,
    pub outcome: Result<WriteOutcome, RecoveryError>,
}

pub(crate) struct RecoveryWriter {
    store: Arc<RecoveryStore>,
    jobs: Option<Sender<WriteJob>>,
    results_tx: Sender<WriteResult>,
    results: Receiver<WriteResult>,
    /// Jobs submitted and not yet finished.
    outstanding: Arc<(Mutex<usize>, Condvar)>,
}

impl RecoveryWriter {
    pub fn new(store: Arc<RecoveryStore>) -> Self {
        let (results_tx, results) = mpsc::channel();
        Self {
            store,
            jobs: None,
            results_tx,
            results,
            outstanding: Arc::new((Mutex::new(0), Condvar::new())),
        }
    }

    /// Queue `job`. `Err` when the worker thread cannot run (nothing was
    /// queued).
    pub fn submit(&mut self, job: WriteJob) -> Result<(), RecoveryError> {
        if self.jobs.is_none() {
            self.jobs = Some(self.spawn()?);
        }
        let sender = self.jobs.as_ref().expect("worker was just started");
        self.bump(1);
        sender.send(job).map_err(|_| {
            self.bump(-1);
            self.jobs = None;
            RecoveryError::Capture {
                detail: "the recovery writer thread stopped".to_string(),
            }
        })
    }

    fn spawn(&self) -> Result<Sender<WriteJob>, RecoveryError> {
        let (jobs_tx, jobs_rx) = mpsc::channel::<WriteJob>();
        let store = Arc::clone(&self.store);
        let results = self.results_tx.clone();
        let outstanding = Arc::clone(&self.outstanding);
        std::thread::Builder::new()
            .name("op-recovery-writer".to_string())
            .spawn(move || {
                for job in jobs_rx {
                    let WriteJob {
                        document,
                        revision,
                        ticket,
                        snapshot,
                        info,
                    } = job;
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        store.write(ticket, &snapshot, &info)
                    }))
                    .unwrap_or_else(|_| {
                        Err(RecoveryError::Document {
                            path: store.dir().display().to_string(),
                            detail: "the writer panicked".to_string(),
                        })
                    });
                    // The snapshot is dropped before the job counts as done.
                    drop(snapshot);
                    let _ = results.send(WriteResult {
                        document,
                        revision,
                        ticket,
                        outcome,
                    });
                    let (count, signal) = &*outstanding;
                    let mut count = count.lock().unwrap_or_else(|p| p.into_inner());
                    *count = count.saturating_sub(1);
                    signal.notify_all();
                }
            })
            .map_err(|error| RecoveryError::Capture {
                detail: format!("could not start the recovery writer: {error}"),
            })?;
        Ok(jobs_tx)
    }

    fn bump(&self, delta: isize) {
        let (count, signal) = &*self.outstanding;
        let mut count = count.lock().unwrap_or_else(|p| p.into_inner());
        *count = count.saturating_add_signed(delta);
        signal.notify_all();
    }

    /// Finished jobs since the last call.
    pub fn drain_results(&self) -> Vec<WriteResult> {
        self.results.try_iter().collect()
    }

    /// Block until every submitted job has finished, or `timeout` passes.
    /// Returns whether the queue is empty.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (count, signal) = &*self.outstanding;
        let mut guard = count.lock().unwrap_or_else(|p| p.into_inner());
        while *guard > 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            guard = match signal.wait_timeout(guard, remaining) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
        true
    }
}
