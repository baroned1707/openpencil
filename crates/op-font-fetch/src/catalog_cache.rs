//! On-disk cache of the Fontsource catalog (24 h). A failed refresh keeps
//! serving the stale copy; with no copy and no network the feature is simply
//! unavailable (callers see `Offline`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::catalog::{Catalog, CATALOG_URL, MAX_CATALOG_BYTES};
use crate::fetch::FetchFailure;
use crate::source::FontSource;

pub const CATALOG_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Serialize, Deserialize)]
struct CacheFile {
    #[serde(rename = "fetchedAtUnix")]
    fetched_at_unix: u64,
    /// Validator the server returned with this copy (diagnostics only).
    etag: Option<String>,
    catalog: Box<RawValue>,
}

/// Where a loaded catalog came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogOrigin {
    /// Cache younger than the TTL; no request was made.
    FreshCache,
    /// Downloaded just now.
    Network,
    /// Older than the TTL, served because refreshing failed (or was not
    /// allowed).
    StaleCache,
}

pub struct LoadedCatalog {
    pub catalog: Catalog,
    pub etag: Option<String>,
    pub origin: CatalogOrigin,
}

pub struct CatalogCache {
    path: PathBuf,
}

impl CatalogCache {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Option<(CacheFile, Catalog)> {
        let bytes = std::fs::read(&self.path).ok()?;
        let file: CacheFile = serde_json::from_slice(&bytes).ok()?;
        let catalog = Catalog::parse(file.catalog.get().as_bytes()).ok()?;
        (!catalog.is_empty()).then_some((file, catalog))
    }

    fn write(&self, fetched_at_unix: u64, etag: Option<String>, body: &[u8]) {
        let Ok(text) = std::str::from_utf8(body) else {
            return;
        };
        let Ok(catalog) = RawValue::from_string(text.to_string()) else {
            return;
        };
        let file = CacheFile {
            fetched_at_unix,
            etag,
            catalog,
        };
        let Ok(json) = serde_json::to_vec(&file) else {
            return;
        };
        if let Some(parent) = self.path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        // Atomic replace so a crash never leaves a half-written cache.
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, &self.path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// Cache only, whatever its age. Never touches the network, so it is what
    /// runs while automatic downloads are switched off.
    pub fn load_cached(&self, now_unix: u64) -> Option<LoadedCatalog> {
        let (file, catalog) = self.read()?;
        let fresh = now_unix.saturating_sub(file.fetched_at_unix) < CATALOG_TTL_SECS;
        Some(LoadedCatalog {
            catalog,
            etag: file.etag,
            origin: if fresh {
                CatalogOrigin::FreshCache
            } else {
                CatalogOrigin::StaleCache
            },
        })
    }

    /// Fresh cache, else the network, else the stale cache.
    pub fn load_or_refresh(
        &self,
        source: &dyn FontSource,
        now_unix: u64,
    ) -> Result<LoadedCatalog, FetchFailure> {
        let cached = self.load_cached(now_unix);
        if let Some(loaded) = cached.as_ref() {
            if loaded.origin == CatalogOrigin::FreshCache {
                return Ok(cached.expect("checked above"));
            }
        }
        let refreshed = source
            .get(CATALOG_URL, MAX_CATALOG_BYTES)
            .map_err(FetchFailure::from)
            .and_then(|response| {
                let catalog = Catalog::parse(&response.bytes)
                    .map_err(|detail| FetchFailure::Invalid(detail))?;
                if catalog.is_empty() {
                    return Err(FetchFailure::Invalid(
                        "catalog has no usable entries".into(),
                    ));
                }
                self.write(now_unix, response.etag.clone(), &response.bytes);
                Ok(LoadedCatalog {
                    catalog,
                    etag: response.etag,
                    origin: CatalogOrigin::Network,
                })
            });
        match (refreshed, cached) {
            (Ok(loaded), _) => Ok(loaded),
            (Err(_), Some(stale)) => Ok(stale),
            (Err(error), None) => Err(error),
        }
    }
}
