//! A shared, bounded memo of `path -> ProjectRoot`.
//!
//! [`project_root_for`](super::project_root_for) reads the disk (two
//! canonicalizes, an `is_dir` and up to six `exists()` per ancestor level), so
//! calling it for every open document on every frame costs real I/O, and a lot
//! of it on a network drive. This cache lets every caller (the tab strip's
//! project name, the Projects dock) share one answer per path.
//!
//! Freshness has two limits so a frame never has to wait for the disk when an
//! answer merely got old:
//!
//! * **soft limit** (30 s): the old answer is still served; the entry is
//!   reported by [`ProjectRootCache::stale_paths`] and a background thread
//!   re-resolves it with [`ProjectRootCache::refresh_stale`].
//! * **hard limit** (5 min): nobody refreshed it (no background thread is
//!   running), so the caller resolves it inline. A newly added `.git` is thus
//!   noticed eventually, whatever the caller.
//!
//! Absent answers (`None`: the parent folder is missing, or the file sits in
//! home or `/`) are cached too, otherwise such a document would hit the disk on
//! every frame. The least recently used entry is dropped when the cache is
//! full. A path is the key as given: canonicalizing it would be the cost this
//! cache exists to avoid, so a path spelled two ways is two entries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{project_root_for, ProjectRoot};

/// Entries kept before the least recently used one is dropped.
pub const DEFAULT_CAPACITY: usize = 256;
/// Age after which an answer is served but queued for a background refresh.
pub const DEFAULT_SOFT_TTL: Duration = Duration::from_secs(30);
/// Age after which an answer is no longer served; the caller resolves inline.
pub const DEFAULT_HARD_TTL: Duration = Duration::from_secs(300);

type Resolver = Box<dyn Fn(&Path) -> Option<ProjectRoot> + Send + Sync>;

struct Entry {
    value: Option<ProjectRoot>,
    resolved_at: Instant,
    /// Use stamp: the entry with the smallest one is the least recently used.
    used: u64,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<PathBuf, Entry>,
    tick: u64,
}

/// See the module docs.
pub struct ProjectRootCache {
    inner: Mutex<Inner>,
    resolver: Resolver,
    capacity: usize,
    soft_ttl: Duration,
    hard_ttl: Duration,
}

impl ProjectRootCache {
    /// A cache with the default limits over `resolver` (the real one is
    /// [`project_root_for`]; tests pass a counting stand-in).
    pub fn new(resolver: impl Fn(&Path) -> Option<ProjectRoot> + Send + Sync + 'static) -> Self {
        Self::with_limits(
            resolver,
            DEFAULT_CAPACITY,
            DEFAULT_SOFT_TTL,
            DEFAULT_HARD_TTL,
        )
    }

    pub fn with_limits(
        resolver: impl Fn(&Path) -> Option<ProjectRoot> + Send + Sync + 'static,
        capacity: usize,
        soft_ttl: Duration,
        hard_ttl: Duration,
    ) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            resolver: Box::new(resolver),
            capacity: capacity.max(1),
            soft_ttl,
            hard_ttl,
        }
    }

    /// The project root of `path`. Touches the disk only when the path was
    /// never resolved or its answer outlived the hard limit.
    pub fn get(&self, path: &Path) -> Option<ProjectRoot> {
        self.get_at(path, Instant::now())
    }

    /// [`Self::get`] with the clock injected.
    pub fn get_at(&self, path: &Path, now: Instant) -> Option<ProjectRoot> {
        {
            let mut inner = self.lock();
            let tick = inner.next_tick();
            if let Some(entry) = inner.entries.get_mut(path) {
                if age(entry, now) < self.hard_ttl {
                    entry.used = tick;
                    return entry.value.clone();
                }
            }
        }
        // Resolved outside the lock: a slow disk must not block other callers.
        let value = (self.resolver)(path);
        self.store(path, value.clone(), now);
        value
    }

    /// Forget `path` (a document moved or was closed, or a folder was just
    /// created under it), so the next [`Self::get`] reads the disk.
    pub fn invalidate(&self, path: &Path) {
        self.lock().entries.remove(path);
    }

    /// Whether an answer for `path` is held (fresh or not).
    pub fn contains(&self, path: &Path) -> bool {
        self.lock().entries.contains_key(path)
    }

    /// Paths whose answer is past the soft limit.
    pub fn stale_paths(&self) -> Vec<PathBuf> {
        self.stale_paths_at(Instant::now())
    }

    pub fn stale_paths_at(&self, now: Instant) -> Vec<PathBuf> {
        self.lock()
            .entries
            .iter()
            .filter(|(_, entry)| age(entry, now) >= self.soft_ttl)
            .map(|(path, _)| path.clone())
            .collect()
    }

    /// Re-resolve every entry past the soft limit; meant for a background
    /// thread. Returns how many were refreshed. An entry dropped meanwhile
    /// (invalidated or evicted) is not brought back.
    pub fn refresh_stale(&self) -> usize {
        self.refresh_stale_at(Instant::now())
    }

    pub fn refresh_stale_at(&self, now: Instant) -> usize {
        let mut refreshed = 0;
        for path in self.stale_paths_at(now) {
            let value = (self.resolver)(&path);
            let mut inner = self.lock();
            if let Some(entry) = inner.entries.get_mut(&path) {
                entry.value = value;
                entry.resolved_at = now;
                refreshed += 1;
            }
        }
        refreshed
    }

    fn store(&self, path: &Path, value: Option<ProjectRoot>, now: Instant) {
        let mut inner = self.lock();
        let tick = inner.next_tick();
        if !inner.entries.contains_key(path) && inner.entries.len() >= self.capacity {
            let oldest = inner
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                inner.entries.remove(&key);
            }
        }
        inner.entries.insert(
            path.to_path_buf(),
            Entry {
                value,
                resolved_at: now,
                used: tick,
            },
        );
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Inner {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
}

fn age(entry: &Entry, now: Instant) -> Duration {
    now.saturating_duration_since(entry.resolved_at)
}

/// The process-wide cache over the real [`project_root_for`].
pub fn shared() -> Arc<ProjectRootCache> {
    static SHARED: OnceLock<Arc<ProjectRootCache>> = OnceLock::new();
    SHARED
        .get_or_init(|| Arc::new(ProjectRootCache::new(project_root_for)))
        .clone()
}

/// [`project_root_for`] through the shared cache: the call for anything that
/// runs per frame.
pub fn cached_project_root_for(path: &Path) -> Option<ProjectRoot> {
    shared().get(path)
}

/// Drop the shared cache's answer for `path` (call when the file at `path` is
/// renamed, saved elsewhere or bound to a new location).
pub fn invalidate_cached_project_root(path: &Path) {
    shared().invalidate(path);
}

#[cfg(test)]
#[path = "project_root_cache_tests.rs"]
mod tests;
