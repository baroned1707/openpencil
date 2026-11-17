//! The Google Fonts catalog the desktop already downloaded, read for feedback.
//!
//! The editor caches the Fontsource catalog at
//! `<config root>/fonts/fontsource-catalog.json` (`op-host-desktop` fonts
//! store; the root is `op_config_store::ConfigStore::user().root()`). Design
//! feedback only ever READS that file, through `CatalogCache::load_cached`
//! (cache only, never the network). A missing, empty or garbled file means "no
//! catalog": feedback then says less, it never fails.
//!
//! The parsed catalog is held process-wide and re-read only when the file's
//! modification time or length changes, so a write pays one `stat`, not a parse.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use op_font_fetch::{Catalog, CatalogCache};

const CATALOG_FILE: &str = "fonts/fontsource-catalog.json";

/// What the last read of one file produced, and which file state it came from.
struct Slot {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
    catalog: Option<Arc<Catalog>>,
}

/// A path-keyed, stat-validated cache of one parsed catalog file.
pub(super) struct CatalogFileCache {
    slot: Mutex<Option<Slot>>,
    /// How many times a file was actually parsed (tests pin "once").
    parses: AtomicUsize,
}

impl CatalogFileCache {
    pub(super) const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            parses: AtomicUsize::new(0),
        }
    }

    /// The catalog stored at `path`, or `None` when the file is missing,
    /// unreadable, garbled or has no usable entry.
    pub(super) fn get(&self, path: &Path) -> Option<Arc<Catalog>> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;
        let len = metadata.len();
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = slot.as_ref() {
            if held.path == path && held.modified == modified && held.len == len {
                return held.catalog.clone();
            }
        }
        self.parses.fetch_add(1, Ordering::Relaxed);
        let catalog = CatalogCache::new(path)
            .load_cached(0)
            .map(|loaded| Arc::new(loaded.catalog));
        *slot = Some(Slot {
            path: path.to_path_buf(),
            modified,
            len,
            catalog: catalog.clone(),
        });
        catalog
    }

    #[cfg(test)]
    pub(super) fn parse_count(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }
}

static PROCESS_CACHE: CatalogFileCache = CatalogFileCache::new();

thread_local! {
    /// Hosts never set this; tests use it to point the lookup at a scratch file
    /// (or at "no file") instead of the developer's real config root.
    static PATH_OVERRIDE: RefCell<Option<Option<PathBuf>>> = const { RefCell::new(None) };
}

/// Run `body` with the catalog lookup on this thread reading `path` (`None`:
/// no catalog file at all).
#[doc(hidden)]
pub fn with_font_catalog_file<R>(path: Option<PathBuf>, body: impl FnOnce() -> R) -> R {
    struct Restore(Option<Option<PathBuf>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PATH_OVERRIDE.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(PATH_OVERRIDE.with(|slot| slot.borrow_mut().replace(path)));
    body()
}

fn catalog_path() -> Option<PathBuf> {
    if let Some(overridden) = PATH_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return overridden;
    }
    let store = op_config_store::ConfigStore::user().ok()?;
    Some(store.root().join(CATALOG_FILE))
}

/// The cached Google Fonts catalog, if the editor has downloaded one.
pub(super) fn cached_catalog() -> Option<Arc<Catalog>> {
    PROCESS_CACHE.get(&catalog_path()?)
}
