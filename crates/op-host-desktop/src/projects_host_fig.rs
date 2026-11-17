//! The queue behind "convert this `.fig` to `.op`" in the Projects dock.
//!
//! This module is the orchestration only: which conversions wait, which run,
//! how long they may run and what a finished one reports. It owns no converter.
//! The real one runs the conversion in a CHILD PROCESS
//! (`projects_host_fig_process.rs`), so a corrupt, malicious or enormous `.fig`
//! can crash or exhaust only that process; tests plug in fakes.
//!
//! Rules every conversion follows:
//! * at most [`FigLimits::max_parallel`] run at once, the rest wait in order;
//! * each one is validated against the disk when it is about to start
//!   (`projects_host_fig_plan.rs`), not when it was queued;
//! * one that runs longer than [`FigLimits::timeout`] is killed and fails with
//!   "timed out"; one that fails never stops the others;
//! * nothing here touches the UI thread's time: [`FigJobs::pump`] only polls
//!   children and starts the next ones.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fig_plan::{plan_conversion, Plan};

/// Children running at once.
pub(crate) const MAX_PARALLEL: usize = 2;
/// A conversion running longer than this is killed.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// An output larger than this is deleted and the conversion fails.
pub(crate) const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Queued entries examined per pump, so a batch of thousands that are all
/// refused cannot stall a frame.
const MAX_PLANS_PER_PUMP: usize = 64;
/// Times one conversion is restarted because its output name was taken in the
/// moment between choosing it and creating it.
const MAX_NAME_RETRIES: u32 = 20;

/// How a conversion ended, as the child reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConvertOutcome {
    /// The output exists, written in full.
    Done,
    /// The output name was taken before the child could create it.
    OutputExists,
    /// The conversion failed; the one-line reason.
    Failed(String),
}

/// One conversion in flight.
pub(crate) trait ConversionRun {
    /// Non-blocking: still running, or how it ended.
    fn poll(&mut self) -> Option<ConvertOutcome>;
    /// Stop it and clean up what it left. Idempotent.
    fn kill(&mut self);
}

/// Starts conversions. `input` and `output` are paths, never shell text.
pub(crate) trait FigConverter: Send + Sync {
    fn start(&self, input: &Path, output: &Path) -> io::Result<Box<dyn ConversionRun>>;
}

/// Limits of the queue; tests shrink them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FigLimits {
    pub max_parallel: usize,
    pub timeout: Duration,
    pub max_output_bytes: u64,
}

impl Default for FigLimits {
    fn default() -> Self {
        Self {
            max_parallel: MAX_PARALLEL,
            timeout: DEFAULT_TIMEOUT,
            max_output_bytes: MAX_OUTPUT_BYTES,
        }
    }
}

/// What the dock shows for a `.fig` that has a job (a file without one is
/// "converted" or "not converted" by what is on disk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JobStatus {
    Converting,
    /// The `.op` exists now; the row says "converted" until the next scan lists
    /// it (see [`FigJobs::clear_done`]), so it never flickers back.
    Done,
    Failed(String),
}

/// A conversion that ended, for the host to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finished {
    /// The listed `.fig`.
    pub fig: PathBuf,
    /// The project root it belongs to.
    pub root: PathBuf,
    /// The `.op` now beside it (written by this job, or found already there).
    pub output: PathBuf,
    /// The user clicked this one file, so the host opens the result in a tab.
    /// Never set by a batch.
    pub open_when_done: bool,
}

struct Queued {
    fig: PathBuf,
    root: PathBuf,
    open_when_done: bool,
    retries: u32,
}

struct Active {
    fig: PathBuf,
    root: PathBuf,
    output: PathBuf,
    open_when_done: bool,
    retries: u32,
    started: Instant,
    run: Box<dyn ConversionRun>,
}

/// The queue of `.fig` conversions of one window.
pub(crate) struct FigJobs {
    converter: Arc<dyn FigConverter>,
    limits: FigLimits,
    queue: VecDeque<Queued>,
    running: Vec<Active>,
    status: BTreeMap<PathBuf, JobStatus>,
    /// Bumped whenever `status` changes, so the tree knows to rebuild.
    version: u64,
}

impl FigJobs {
    pub(crate) fn new(converter: Arc<dyn FigConverter>) -> Self {
        Self::with_limits(converter, FigLimits::default())
    }

    pub(crate) fn with_limits(converter: Arc<dyn FigConverter>, limits: FigLimits) -> Self {
        Self {
            converter,
            limits,
            queue: VecDeque::new(),
            running: Vec::new(),
            status: BTreeMap::new(),
            version: 0,
        }
    }

    /// Replace the converter (tests inject a fake).
    #[cfg(test)]
    pub(crate) fn set_converter(&mut self, converter: Arc<dyn FigConverter>) {
        self.converter = converter;
    }

    #[cfg(test)]
    pub(crate) fn set_limits(&mut self, limits: FigLimits) {
        self.limits = limits;
    }

    /// Queue the conversion of `fig` (listed under `root`). A file that is
    /// already queued or running is left alone. Returns whether it was queued.
    pub(crate) fn enqueue(&mut self, fig: PathBuf, root: PathBuf, open_when_done: bool) -> bool {
        if matches!(self.status.get(&fig), Some(JobStatus::Converting)) {
            return false;
        }
        self.set_status(&fig, Some(JobStatus::Converting));
        self.queue.push_back(Queued {
            fig,
            root,
            open_when_done,
            retries: 0,
        });
        true
    }

    /// Whether a running conversion is writing `path` (its `.op` output), so
    /// the file must not be renamed or touched until it ends.
    pub(crate) fn is_writing(&self, path: &Path) -> bool {
        self.running.iter().any(|active| active.output == path)
    }

    /// Whether anything is queued or running (the host keeps waking while so).
    pub(crate) fn has_work(&self) -> bool {
        !self.queue.is_empty() || !self.running.is_empty()
    }

    /// Conversions running right now.
    #[cfg(test)]
    pub(crate) fn running_count(&self) -> usize {
        self.running.len()
    }

    pub(crate) fn status(&self) -> &BTreeMap<PathBuf, JobStatus> {
        &self.status
    }

    pub(crate) fn version(&self) -> u64 {
        self.version
    }

    /// Drop everything queued for `root` and stop what runs for it (the
    /// project was closed). Their rows simply lose the status.
    pub(crate) fn forget_root(&mut self, root: &Path) {
        self.queue.retain(|queued| queued.root != root);
        let mut kept = Vec::new();
        for mut active in std::mem::take(&mut self.running) {
            if active.root == root {
                active.run.kill();
            } else {
                kept.push(active);
            }
        }
        self.running = kept;
        let before = self.status.len();
        let live: Vec<PathBuf> = self
            .queue
            .iter()
            .map(|queued| queued.fig.clone())
            .chain(self.running.iter().map(|active| active.fig.clone()))
            .collect();
        // A deeper opened project inside `root` keeps its own queued files.
        self.status
            .retain(|fig, _| !fig.starts_with(root) || live.contains(fig));
        if self.status.len() != before {
            self.version += 1;
        }
    }

    /// Forget the "done" marks under `root`: a scan that started after those
    /// conversions finished has listed their outputs.
    pub(crate) fn clear_done(&mut self, root: &Path) {
        let before = self.status.len();
        self.status
            .retain(|fig, status| !(fig.starts_with(root) && *status == JobStatus::Done));
        if self.status.len() != before {
            self.version += 1;
        }
    }

    /// One poll: finish what ended or timed out, then start queued conversions
    /// while there is room. Never blocks. `now` is injectable for tests.
    pub(crate) fn pump(&mut self, now: Instant) -> Vec<Finished> {
        let mut finished = Vec::new();
        let mut still_running = Vec::new();
        for mut active in std::mem::take(&mut self.running) {
            let outcome = if now.saturating_duration_since(active.started) >= self.limits.timeout {
                active.run.kill();
                Some(ConvertOutcome::Failed("timed out".to_string()))
            } else {
                active.run.poll()
            };
            match outcome {
                None => still_running.push(active),
                Some(outcome) => self.settle(active, outcome, &mut finished),
            }
        }
        self.running = still_running;
        self.start_queued(now, &mut finished);
        finished
    }

    fn settle(&mut self, active: Active, outcome: ConvertOutcome, finished: &mut Vec<Finished>) {
        match outcome {
            ConvertOutcome::Done => {
                let too_large = std::fs::metadata(&active.output)
                    .is_ok_and(|meta| meta.len() > self.limits.max_output_bytes);
                if too_large {
                    // The file is the one this job just created.
                    let _ = std::fs::remove_file(&active.output);
                    self.set_status(
                        &active.fig,
                        Some(JobStatus::Failed("output too large".to_string())),
                    );
                    return;
                }
                self.set_status(&active.fig, Some(JobStatus::Done));
                finished.push(Finished {
                    fig: active.fig,
                    root: active.root,
                    output: active.output,
                    open_when_done: active.open_when_done,
                });
            }
            ConvertOutcome::OutputExists if active.retries < MAX_NAME_RETRIES => {
                // Someone took the name first: plan again, the next one is free.
                self.queue.push_front(Queued {
                    fig: active.fig,
                    root: active.root,
                    open_when_done: active.open_when_done,
                    retries: active.retries + 1,
                });
            }
            ConvertOutcome::OutputExists => self.set_status(
                &active.fig,
                Some(JobStatus::Failed("no free output name".to_string())),
            ),
            ConvertOutcome::Failed(reason) => {
                self.set_status(&active.fig, Some(JobStatus::Failed(reason)));
            }
        }
    }

    fn start_queued(&mut self, now: Instant, finished: &mut Vec<Finished>) {
        let mut examined = 0;
        while self.running.len() < self.limits.max_parallel && examined < MAX_PLANS_PER_PUMP {
            let Some(queued) = self.queue.pop_front() else {
                break;
            };
            examined += 1;
            match plan_conversion(&queued.root, &queued.fig) {
                Err(refusal) => {
                    self.set_status(&queued.fig, Some(JobStatus::Failed(refusal.to_string())));
                }
                Ok(Plan::AlreadyConverted(output)) => {
                    self.set_status(&queued.fig, Some(JobStatus::Done));
                    finished.push(Finished {
                        fig: queued.fig,
                        root: queued.root,
                        output,
                        open_when_done: queued.open_when_done,
                    });
                }
                Ok(Plan::Convert { source, output }) => {
                    match self.converter.start(&source, &output) {
                        Ok(run) => self.running.push(Active {
                            fig: queued.fig,
                            root: queued.root,
                            output,
                            open_when_done: queued.open_when_done,
                            retries: queued.retries,
                            started: now,
                            run,
                        }),
                        Err(error) => self.set_status(
                            &queued.fig,
                            Some(JobStatus::Failed(format!("cannot start: {error}"))),
                        ),
                    }
                }
            }
        }
    }

    fn set_status(&mut self, fig: &Path, status: Option<JobStatus>) {
        let changed = match status {
            Some(status) => self.status.insert(fig.to_path_buf(), status.clone()) != Some(status),
            None => self.status.remove(fig).is_some(),
        };
        if changed {
            self.version += 1;
        }
    }
}

impl Drop for FigJobs {
    /// Quitting stops every child; none outlives the app.
    fn drop(&mut self) {
        for active in &mut self.running {
            active.run.kill();
        }
    }
}

#[cfg(test)]
#[path = "projects_host_fig_tests.rs"]
mod tests;
