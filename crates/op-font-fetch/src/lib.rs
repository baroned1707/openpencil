//! Core of the "download missing Google Fonts" feature (desktop only).
//!
//! Sources, both HTTPS and key-less: the Fontsource catalog
//! (`api.fontsource.org/v1/fonts`) decides whether a family is a known Google
//! family under an allowed licence; the files themselves come from the
//! google/fonts repository on `raw.githubusercontent.com`, located and
//! cross-checked through each family's `METADATA.pb`.
//!
//! The crate is skia-free and network-free: all I/O goes through
//! [`FontSource`], so every rule here is tested against [`fake`].

pub mod catalog;
pub mod catalog_cache;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod fetch;
pub mod license;
pub mod metadata;
pub mod plan;
pub mod session;
pub mod source;
pub mod urls;

pub use catalog::{normalize_family, Catalog, CatalogEntry};
pub use catalog_cache::{CatalogCache, CatalogOrigin, LoadedCatalog};
pub use fetch::{
    classify, fetch_family, looks_like_truetype, DownloadedFile, FamilyDownload, FetchFailure,
    FetchLimits,
};
pub use license::License;
pub use plan::FaceRequest;
pub use session::{FamilyStatus, FontFetcher, SessionConfig, StatusChange};
pub use source::{FetchError, FontSource, Response};

#[cfg(test)]
mod tests;
