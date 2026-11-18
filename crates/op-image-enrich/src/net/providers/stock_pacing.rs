//! Rate-limit and cache state of the key-based stock providers (Pixabay,
//! Unsplash). Both structures are pure and take `now: Instant`, so the tests
//! drive them with a fake clock instead of sleeping.
//!
//! One [`StockRuntime`] per provider base URL lives in a process-wide
//! registry, so two providers (or a real host and a loopback test fake)
//! never share a window or a cache.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// At most `limit` requests per `window`, plus an optional block (a server's
/// `Retry-After`) during which nothing is sent.
#[derive(Debug)]
pub(crate) struct SlidingWindow {
    limit: usize,
    window: Duration,
    stamps: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

impl SlidingWindow {
    pub(crate) fn new(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            stamps: VecDeque::new(),
            blocked_until: None,
        }
    }

    /// Reserve one request slot at `now`; `false` when the window is full or
    /// the provider asked us to wait.
    pub(crate) fn try_acquire_at(&mut self, now: Instant) -> bool {
        if self.blocked_until.is_some_and(|until| now < until) {
            return false;
        }
        while self
            .stamps
            .front()
            .is_some_and(|stamp| now.saturating_duration_since(*stamp) >= self.window)
        {
            self.stamps.pop_front();
        }
        if self.stamps.len() >= self.limit {
            return false;
        }
        self.stamps.push_back(now);
        true
    }

    /// Send nothing until `now + wait` (never shortens an existing block).
    pub(crate) fn block_for_at(&mut self, now: Instant, wait: Duration) {
        let until = now + wait;
        self.blocked_until = Some(self.blocked_until.map_or(until, |old| old.max(until)));
    }
}

/// Query answers kept for a fixed time, bounded in size.
#[derive(Debug)]
pub(crate) struct TtlCache<V> {
    ttl: Duration,
    capacity: usize,
    entries: HashMap<String, (Instant, V)>,
}

impl<V: Clone> TtlCache<V> {
    pub(crate) fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity,
            entries: HashMap::new(),
        }
    }

    pub(crate) fn get_at(&self, key: &str, now: Instant) -> Option<V> {
        let (stored_at, value) = self.entries.get(key)?;
        (now.saturating_duration_since(*stored_at) < self.ttl).then(|| value.clone())
    }

    pub(crate) fn insert_at(&mut self, key: String, value: V, now: Instant) {
        self.entries
            .retain(|_, (stored_at, _)| now.saturating_duration_since(*stored_at) < self.ttl);
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (stored_at, _))| *stored_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (now, value));
    }
}

/// Window + cache of one provider host.
pub(crate) struct StockRuntime {
    pub(crate) window: Mutex<SlidingWindow>,
    pub(crate) cache: Mutex<TtlCache<serde_json::Value>>,
}

impl StockRuntime {
    pub(crate) fn new(limit: usize, window: Duration, ttl: Duration, capacity: usize) -> Self {
        Self {
            window: Mutex::new(SlidingWindow::new(limit, window)),
            cache: Mutex::new(TtlCache::new(ttl, capacity)),
        }
    }
}

/// The registry entry for `base_url`, created with the given limits on first
/// use.
pub(crate) fn runtime_for(
    base_url: &str,
    limit: usize,
    window: Duration,
    ttl: Duration,
    capacity: usize,
) -> Arc<StockRuntime> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<StockRuntime>>>> = OnceLock::new();
    let mut registry = REGISTRY
        .get_or_init(Default::default)
        .lock()
        .expect("stock runtime registry poisoned");
    Arc::clone(
        registry
            .entry(base_url.to_string())
            .or_insert_with(|| Arc::new(StockRuntime::new(limit, window, ttl, capacity))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_admits_the_limit_then_refuses_until_the_window_slides() {
        let start = Instant::now();
        let mut window = SlidingWindow::new(3, Duration::from_secs(60));
        assert!((0..3).all(|_| window.try_acquire_at(start)));
        assert!(!window.try_acquire_at(start + Duration::from_secs(59)));
        assert!(window.try_acquire_at(start + Duration::from_secs(60)));
    }

    #[test]
    fn retry_after_block_wins_over_a_free_window() {
        let start = Instant::now();
        let mut window = SlidingWindow::new(100, Duration::from_secs(60));
        window.block_for_at(start, Duration::from_secs(30));
        assert!(!window.try_acquire_at(start + Duration::from_secs(29)));
        assert!(window.try_acquire_at(start + Duration::from_secs(30)));
    }

    #[test]
    fn cache_entries_expire_and_the_oldest_is_evicted_at_capacity() {
        let start = Instant::now();
        let mut cache: TtlCache<u32> = TtlCache::new(Duration::from_secs(10), 2);
        cache.insert_at("a".into(), 1, start);
        cache.insert_at("b".into(), 2, start + Duration::from_secs(1));
        cache.insert_at("c".into(), 3, start + Duration::from_secs(2));
        assert_eq!(cache.get_at("a", start + Duration::from_secs(2)), None);
        assert_eq!(cache.get_at("c", start + Duration::from_secs(2)), Some(3));
        assert_eq!(cache.get_at("c", start + Duration::from_secs(12)), None);
    }
}
