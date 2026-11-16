//! Project-root cache: it answers from memory, forgets on request, expires
//! and stays bounded.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn counting_cache(
    capacity: usize,
    soft: Duration,
    hard: Duration,
) -> (ProjectRootCache, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let cache = ProjectRootCache::with_limits(
        move |path: &Path| {
            seen.fetch_add(1, Ordering::SeqCst);
            path.parent().map(|parent| ProjectRoot {
                root: parent.to_path_buf(),
                name: "p".to_string(),
                source: super::super::ProjectRootSource::FileDir,
            })
        },
        capacity,
        soft,
        hard,
    );
    (cache, calls)
}

fn calls(counter: &AtomicUsize) -> usize {
    counter.load(Ordering::SeqCst)
}

#[test]
fn a_cached_path_is_not_resolved_again() {
    let (cache, resolved) = counting_cache(8, DEFAULT_SOFT_TTL, DEFAULT_HARD_TTL);
    let path = Path::new("/p/a.op");
    let first = cache.get(path);
    assert!(first.is_some());
    assert_eq!(cache.get(path), first);
    assert_eq!(cache.get(path), first);
    assert_eq!(calls(&resolved), 1);
}

#[test]
fn an_absent_answer_is_cached_too() {
    let (cache, resolved) = counting_cache(8, DEFAULT_SOFT_TTL, DEFAULT_HARD_TTL);
    // `/` has no parent, so the stand-in resolver answers `None`.
    assert!(cache.get(Path::new("/")).is_none());
    assert!(cache.get(Path::new("/")).is_none());
    assert_eq!(calls(&resolved), 1, "a None answer must not be re-resolved");
}

#[test]
fn invalidate_makes_the_next_get_read_again() {
    let (cache, resolved) = counting_cache(8, DEFAULT_SOFT_TTL, DEFAULT_HARD_TTL);
    let path = Path::new("/p/a.op");
    cache.get(path);
    assert!(cache.contains(path));
    cache.invalidate(path);
    assert!(!cache.contains(path));
    cache.get(path);
    assert_eq!(calls(&resolved), 2);
}

#[test]
fn root_cache_entries_expire() {
    let (cache, resolved) = counting_cache(8, Duration::from_secs(30), Duration::from_secs(300));
    let path = Path::new("/p/a.op");
    let start = Instant::now();
    cache.get_at(path, start);
    let later = start + Duration::from_secs(31);
    assert!(cache
        .stale_paths_at(start + Duration::from_secs(29))
        .is_empty());
    assert_eq!(cache.stale_paths_at(later), vec![path.to_path_buf()]);
    assert!(cache.get_at(path, later).is_some());
    assert_eq!(calls(&resolved), 1, "a stale answer is served without I/O");
    assert_eq!(cache.refresh_stale_at(later), 1);
    assert_eq!(calls(&resolved), 2);
    assert!(
        cache.stale_paths_at(later).is_empty(),
        "the refresh restarts the clock"
    );
}

#[test]
fn entries_past_the_hard_limit_are_resolved_inline() {
    let (cache, resolved) = counting_cache(8, Duration::from_secs(30), Duration::from_secs(300));
    let path = Path::new("/p/a.op");
    let start = Instant::now();
    cache.get_at(path, start);
    cache.get_at(path, start + Duration::from_secs(299));
    assert_eq!(calls(&resolved), 1);
    cache.get_at(path, start + Duration::from_secs(301));
    assert_eq!(
        calls(&resolved),
        2,
        "nobody refreshed it, so the caller does"
    );
}

#[test]
fn the_least_recently_used_entry_is_dropped_when_full() {
    let (cache, _) = counting_cache(2, DEFAULT_SOFT_TTL, DEFAULT_HARD_TTL);
    let (a, b, c) = (
        Path::new("/p/a.op"),
        Path::new("/p/b.op"),
        Path::new("/p/c.op"),
    );
    cache.get(a);
    cache.get(b);
    cache.get(a); // a is now more recent than b
    cache.get(c);
    assert!(cache.contains(a));
    assert!(!cache.contains(b), "b was the least recently used");
    assert!(cache.contains(c));
}

#[test]
fn a_refresh_does_not_resurrect_an_invalidated_entry() {
    let (cache, _) = counting_cache(8, Duration::from_secs(1), Duration::from_secs(300));
    let path = Path::new("/p/a.op");
    let start = Instant::now();
    cache.get_at(path, start);
    cache.invalidate(path);
    assert_eq!(cache.refresh_stale_at(start + Duration::from_secs(5)), 0);
    assert!(!cache.contains(path));
}

#[test]
fn the_shared_cache_matches_the_uncached_resolution() {
    let dir = std::env::temp_dir().join(format!("op-root-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    let file = dir.join("a.op");
    assert_eq!(cached_project_root_for(&file), project_root_for(&file));
    invalidate_cached_project_root(&file);
    let _ = std::fs::remove_dir_all(dir);
}
