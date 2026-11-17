//! The per-process download scheduler. It owns the status of every family
//! asked about this session (which doubles as the negative cache), runs at
//! most `max_concurrent` jobs on detached worker threads, and hands finished
//! downloads back to the UI thread inside [`FontFetcher::poll`], where the
//! caller stores and registers them (font registration must stay on the UI
//! thread; workers only fetch and check bytes).

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::catalog::{normalize_family, Catalog};
use crate::catalog_cache::CatalogCache;
use crate::fetch::{classify, fetch_family, FamilyDownload, FetchFailure, FetchLimits};
use crate::plan::FaceRequest;
use crate::source::FontSource;

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub max_concurrent: usize,
    /// Soft cap on bytes downloaded this session; checked before each family.
    pub byte_budget: usize,
    pub max_file_bytes: usize,
    pub family_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 2,
            byte_budget: 64 * 1024 * 1024,
            max_file_bytes: 16 * 1024 * 1024,
            family_timeout: Duration::from_secs(180),
        }
    }
}

/// What the session knows about one family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyStatus {
    /// May be downloaded (or downloadability is not known yet); idle.
    Ready,
    Downloading,
    Downloaded,
    Failed(FetchFailure),
}

/// A status transition for the UI. `family` is the spelling the caller used.
pub type StatusChange = (String, FamilyStatus);

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    /// Catalog check from the on-disk cache only; never touches the network.
    Probe,
    Download,
}

struct Job {
    family: String,
    faces: BTreeSet<FaceRequest>,
    kind: JobKind,
}

enum Output {
    Probed(Result<(), FetchFailure>),
    Downloaded(Result<FamilyDownload, FetchFailure>),
}

struct Finished {
    key: String,
    family: String,
    output: Output,
}

#[derive(Default)]
struct CatalogSlot {
    catalog: Option<Arc<Catalog>>,
    /// True once `load_or_refresh` ran this session (even if it had to serve
    /// a stale copy): later downloads reuse it instead of hammering the API.
    refreshed: bool,
    failed: Option<FetchFailure>,
}

pub struct FontFetcher {
    source: Arc<dyn FontSource>,
    cache: Arc<CatalogCache>,
    config: SessionConfig,
    now_unix: fn() -> u64,
    slot: Arc<Mutex<CatalogSlot>>,
    statuses: HashMap<String, FamilyStatus>,
    probing: HashSet<String>,
    queue: VecDeque<(String, Job)>,
    in_flight: usize,
    sender: Sender<Finished>,
    receiver: Receiver<Finished>,
    bytes_downloaded: usize,
    deferred_changes: Vec<StatusChange>,
}

fn system_now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl FontFetcher {
    pub fn new(source: Arc<dyn FontSource>, cache: CatalogCache, config: SessionConfig) -> Self {
        Self::with_clock(source, cache, config, system_now_unix)
    }

    pub fn with_clock(
        source: Arc<dyn FontSource>,
        cache: CatalogCache,
        config: SessionConfig,
        now_unix: fn() -> u64,
    ) -> Self {
        let (sender, receiver) = channel();
        Self {
            source,
            cache: Arc::new(cache),
            config,
            now_unix,
            slot: Arc::new(Mutex::new(CatalogSlot::default())),
            statuses: HashMap::new(),
            probing: HashSet::new(),
            queue: VecDeque::new(),
            in_flight: 0,
            sender,
            receiver,
            bytes_downloaded: 0,
            deferred_changes: Vec::new(),
        }
    }

    pub fn status(&self, family: &str) -> Option<&FamilyStatus> {
        normalize_family(family).and_then(|key| self.statuses.get(&key))
    }

    /// True while any job is queued or running (the host keeps polling).
    pub fn is_busy(&self) -> bool {
        self.in_flight > 0 || !self.queue.is_empty()
    }

    pub fn bytes_downloaded(&self) -> usize {
        self.bytes_downloaded
    }

    /// Classify a family from the cached catalog without any network access,
    /// so the UI can say "unknown font" / "licence not allowed" while
    /// automatic downloads are off. No cache means the status stays unknown.
    pub fn probe(&mut self, family: &str) {
        let Some(key) = normalize_family(family) else {
            self.set_deferred(
                family,
                FamilyStatus::Failed(FetchFailure::UnknownFont),
                &key_of(family),
            );
            return;
        };
        if self.statuses.contains_key(&key) || !self.probing.insert(key.clone()) {
            return;
        }
        self.queue.push_back((
            key,
            Job {
                family: family.to_string(),
                faces: BTreeSet::new(),
                kind: JobKind::Probe,
            },
        ));
        self.pump();
    }

    /// Queue a download of `faces` of `family`. A family that already failed
    /// (the session's negative cache), is downloading or is downloaded is not
    /// asked again unless `force` is set, which is what the user's Download /
    /// Retry click passes. Returns whether a job was queued.
    pub fn request_download(
        &mut self,
        family: &str,
        faces: BTreeSet<FaceRequest>,
        force: bool,
    ) -> bool {
        let Some(key) = normalize_family(family) else {
            self.set_deferred(
                family,
                FamilyStatus::Failed(FetchFailure::UnknownFont),
                &key_of(family),
            );
            return false;
        };
        match self.statuses.get(&key) {
            Some(FamilyStatus::Downloading) => return false,
            Some(FamilyStatus::Downloaded) if !force => return false,
            Some(FamilyStatus::Failed(_)) if !force => return false,
            Some(FamilyStatus::Failed(failure)) if !failure.retryable() => return false,
            _ => {}
        }
        if force {
            if let Ok(mut slot) = self.slot.lock() {
                slot.failed = None;
            }
        }
        self.statuses.insert(key.clone(), FamilyStatus::Downloading);
        self.deferred_changes
            .push((family.to_string(), FamilyStatus::Downloading));
        self.queue.push_back((
            key,
            Job {
                family: family.to_string(),
                faces,
                kind: JobKind::Download,
            },
        ));
        self.pump();
        true
    }

    fn set_deferred(&mut self, family: &str, status: FamilyStatus, key: &str) {
        self.statuses.insert(key.to_string(), status.clone());
        self.deferred_changes.push((family.to_string(), status));
    }

    fn pump(&mut self) {
        while self.in_flight < self.config.max_concurrent.max(1) {
            let Some((key, job)) = self.queue.pop_front() else {
                break;
            };
            if job.kind == JobKind::Download && self.bytes_downloaded >= self.config.byte_budget {
                let status = FamilyStatus::Failed(FetchFailure::BudgetExceeded);
                self.statuses.insert(key, status.clone());
                self.deferred_changes.push((job.family, status));
                continue;
            }
            self.in_flight += 1;
            let source = Arc::clone(&self.source);
            let cache = Arc::clone(&self.cache);
            let slot = Arc::clone(&self.slot);
            let sender = self.sender.clone();
            let config = self.config;
            let now_unix = self.now_unix;
            std::thread::spawn(move || {
                let output = run_job(&*source, &cache, &slot, now_unix, &config, &job);
                let _ = sender.send(Finished {
                    key,
                    family: job.family,
                    output,
                });
            });
        }
    }

    /// Collect finished work on the UI thread. `register` stores and registers
    /// one downloaded family; its error becomes that family's failure.
    pub fn poll(
        &mut self,
        register: &mut dyn FnMut(&FamilyDownload) -> Result<(), FetchFailure>,
    ) -> Vec<StatusChange> {
        let mut changes = std::mem::take(&mut self.deferred_changes);
        loop {
            let finished = match self.receiver.try_recv() {
                Ok(finished) => finished,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            };
            self.in_flight = self.in_flight.saturating_sub(1);
            let Finished {
                key,
                family,
                output,
            } = finished;
            let status = match output {
                Output::Probed(result) => {
                    self.probing.remove(&key);
                    if self.statuses.contains_key(&key) {
                        continue;
                    }
                    match result {
                        Ok(()) => FamilyStatus::Ready,
                        Err(failure) => FamilyStatus::Failed(failure),
                    }
                }
                Output::Downloaded(Ok(download)) => {
                    self.bytes_downloaded += download.total_bytes();
                    match register(&download) {
                        Ok(()) => FamilyStatus::Downloaded,
                        Err(failure) => FamilyStatus::Failed(failure),
                    }
                }
                Output::Downloaded(Err(failure)) => FamilyStatus::Failed(failure),
            };
            self.statuses.insert(key, status.clone());
            changes.push((family, status));
        }
        self.pump();
        changes.append(&mut self.deferred_changes);
        changes
    }
}

fn key_of(family: &str) -> String {
    family.trim().to_lowercase()
}

/// The catalog for a probe: whatever is in memory or on disk, never the network.
fn cached_catalog(
    slot: &Mutex<CatalogSlot>,
    cache: &CatalogCache,
    now_unix: u64,
) -> Option<Arc<Catalog>> {
    let mut slot = slot.lock().ok()?;
    if slot.catalog.is_none() {
        slot.catalog = cache
            .load_cached(now_unix)
            .map(|loaded| Arc::new(loaded.catalog));
    }
    slot.catalog.clone()
}

/// The catalog for a download: refreshed (or fresh-cached) once per session.
fn refreshed_catalog(
    slot: &Mutex<CatalogSlot>,
    cache: &CatalogCache,
    source: &dyn FontSource,
    now_unix: u64,
) -> Result<Arc<Catalog>, FetchFailure> {
    let mut slot = slot
        .lock()
        .map_err(|_| FetchFailure::Other("catalog lock poisoned".into()))?;
    if slot.refreshed {
        if let Some(catalog) = slot.catalog.clone() {
            return Ok(catalog);
        }
    }
    if let Some(failure) = slot.failed.clone() {
        return Err(failure);
    }
    match cache.load_or_refresh(source, now_unix) {
        Ok(loaded) => {
            let catalog = Arc::new(loaded.catalog);
            slot.catalog = Some(Arc::clone(&catalog));
            slot.refreshed = true;
            Ok(catalog)
        }
        Err(failure) => {
            slot.failed = Some(failure.clone());
            Err(failure)
        }
    }
}

fn run_job(
    source: &dyn FontSource,
    cache: &CatalogCache,
    slot: &Mutex<CatalogSlot>,
    now_unix: fn() -> u64,
    config: &SessionConfig,
    job: &Job,
) -> Output {
    match job.kind {
        JobKind::Probe => Output::Probed(match cached_catalog(slot, cache, now_unix()) {
            Some(catalog) => classify(&catalog, &job.family).map(|_| ()),
            None => Ok(()),
        }),
        JobKind::Download => Output::Downloaded((|| {
            let catalog = refreshed_catalog(slot, cache, source, now_unix())?;
            let entry = classify(&catalog, &job.family)?;
            let limits = FetchLimits {
                max_file_bytes: config.max_file_bytes,
                deadline: Instant::now() + config.family_timeout,
            };
            fetch_family(source, entry, &job.faces, &limits)
        })()),
    }
}
