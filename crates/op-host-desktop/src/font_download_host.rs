//! Desktop driver of the missing-font downloader.
//!
//! The rules (catalog, licence gate, planning, scheduling) live in
//! `op-font-fetch` and are network-free; this module is the thin app side that
//! once per frame:
//!   1. turns the user's Download / Download all clicks into requests,
//!   2. when "Download missing fonts automatically" is on, requests every
//!      missing row that has not been tried this session (the fetcher's status
//!      map is the negative cache, so a document re-open or re-detection never
//!      refetches a failure),
//!   3. when it is off, only classifies rows from the cached catalog (no
//!      network) so a row can say "not a Google Fonts family" up front,
//!   4. collects finished downloads on the UI thread, stores + registers them
//!      (`FontStore`), and refreshes the imported-font snapshot and the prompt.
//!
//! Workers only fetch bytes; font registration stays on the UI thread.

use std::collections::BTreeSet;
use std::sync::Arc;

use op_editor_core::font_download::{FontDownloadFailure, FontDownloadRequest, FontDownloadStatus};
use op_editor_core::missing_font_faces::document_font_faces;
use op_font_fetch::{
    CatalogCache, FaceRequest, FamilyDownload, FamilyStatus, FetchFailure, FontFetcher, FontSource,
    SessionConfig,
};
use op_host_native::WidgetHostNative;

use crate::fonts::FontStore;

#[path = "font_download_parked.rs"]
mod parked;

pub(crate) struct FontDownloadHost {
    fetcher: FontFetcher,
    store: Option<FontStore>,
    /// Which parked documents were last checked for missing families.
    parked_scans: parked::ParkedFontScans,
}

fn failure_kind(failure: &FetchFailure) -> FontDownloadFailure {
    match failure {
        FetchFailure::UnknownFont => FontDownloadFailure::UnknownFont,
        FetchFailure::LicenseNotAllowed => FontDownloadFailure::LicenseNotAllowed,
        FetchFailure::Offline => FontDownloadFailure::Offline,
        FetchFailure::Timeout => FontDownloadFailure::Timeout,
        FetchFailure::NotFound => FontDownloadFailure::NotFound,
        FetchFailure::TooLarge => FontDownloadFailure::TooLarge,
        FetchFailure::Invalid(_) => FontDownloadFailure::Invalid,
        FetchFailure::BudgetExceeded => FontDownloadFailure::BudgetExceeded,
        FetchFailure::Http(_) | FetchFailure::Other(_) => FontDownloadFailure::Other,
    }
}

fn ui_status(status: &FamilyStatus) -> FontDownloadStatus {
    match status {
        FamilyStatus::Ready => FontDownloadStatus::Ready,
        FamilyStatus::Downloading => FontDownloadStatus::Downloading,
        FamilyStatus::Downloaded => FontDownloadStatus::Downloaded,
        FamilyStatus::Failed(failure) => FontDownloadStatus::Failed(failure_kind(failure)),
    }
}

impl FontDownloadHost {
    /// Production: the real HTTPS source and the per-user font store.
    pub(crate) fn new() -> Self {
        // Unit tests that build a whole `DesktopApp` must never reach the
        // network or the developer's font store.
        if cfg!(test) {
            return Self::unavailable(None);
        }
        let store = FontStore::user().ok();
        let source: Option<Arc<dyn FontSource>> =
            crate::font_download_source::HttpsFontSource::new()
                .ok()
                .map(|source| Arc::new(source) as Arc<dyn FontSource>);
        match (source, store) {
            (Some(source), Some(store)) => Self::with_parts(source, store),
            // No client or no config dir: the feature quietly stays off.
            (_, store) => Self::unavailable(store),
        }
    }

    /// Tests and `new`: any source, any store root.
    pub(crate) fn with_parts(source: Arc<dyn FontSource>, store: FontStore) -> Self {
        let cache = CatalogCache::new(store.catalog_cache_path());
        Self {
            fetcher: FontFetcher::new(source, cache, SessionConfig::default()),
            store: Some(store),
            parked_scans: Default::default(),
        }
    }

    fn unavailable(store: Option<FontStore>) -> Self {
        struct Never;
        impl FontSource for Never {
            fn get(
                &self,
                _: &str,
                _: usize,
            ) -> Result<op_font_fetch::Response, op_font_fetch::FetchError> {
                Err(op_font_fetch::FetchError::Offline)
            }
        }
        let cache = CatalogCache::new(std::env::temp_dir().join("openpencil-no-font-catalog.json"));
        Self {
            fetcher: FontFetcher::new(Arc::new(Never), cache, SessionConfig::default()),
            store,
            parked_scans: Default::default(),
        }
    }

    /// Whether a worker is running or queued (keeps the event loop polling).
    pub(crate) fn is_busy(&self) -> bool {
        self.fetcher.is_busy()
    }

    /// One frame of work. Returns `true` when something visible changed.
    pub(crate) fn drain(&mut self, host: &mut WidgetHostNative) -> bool {
        let mut changed = false;
        let (entries, auto) = {
            let ui = &host.editor_state().editor_ui;
            let entries = ui
                .missing_fonts_prompt
                .as_ref()
                .map(|prompt| prompt.entries.clone())
                .unwrap_or_default();
            (entries, ui.agent_settings.auto_download_missing_fonts)
        };

        // 1. The user's clicks. A click forces past the negative cache.
        let requests =
            std::mem::take(&mut host.editor_state_mut().editor_ui.font_downloads.pending);
        for request in requests {
            match request {
                FontDownloadRequest::Family(family) => {
                    changed |= self.request(host, &family, true);
                }
                FontDownloadRequest::All => {
                    for entry in entries.iter().filter(|entry| !entry.resolved) {
                        let allowed = host
                            .editor_state()
                            .editor_ui
                            .font_downloads
                            .row_action(entry)
                            .is_some();
                        if allowed {
                            changed |= self.request(host, &entry.family, true);
                        }
                    }
                }
            }
        }

        // 2./3. Rows nobody has looked at yet.
        if host.editor_state().editor_ui.system_fonts_loaded {
            for entry in entries.iter().filter(|entry| !entry.resolved) {
                let untouched = matches!(
                    self.fetcher.status(&entry.family),
                    None | Some(FamilyStatus::Ready)
                );
                if !untouched {
                    continue;
                }
                if auto {
                    changed |= self.request(host, &entry.family, false);
                } else if self.fetcher.status(&entry.family).is_none() {
                    self.fetcher.probe(&entry.family);
                }
            }
        }

        // 4. Finished work.
        changed |= self.collect(host);
        changed | self.demote_unresolved_downloads(host)
    }

    fn request(&mut self, host: &WidgetHostNative, family: &str, force: bool) -> bool {
        let faces: BTreeSet<FaceRequest> = document_font_faces(host.editor_state(), family)
            .into_iter()
            .map(|(weight, italic)| FaceRequest { weight, italic })
            .collect();
        self.fetcher.request_download(family, faces, force)
    }

    /// Poll the workers, register what finished, mirror every status change
    /// into the UI state, and refresh the rows when a family became available.
    fn collect(&mut self, host: &mut WidgetHostNative) -> bool {
        let store = self.store.as_ref();
        let mut register = |download: &FamilyDownload| -> Result<(), FetchFailure> {
            let store =
                store.ok_or_else(|| FetchFailure::Other("font store unavailable".into()))?;
            store.store_downloaded_family(download).map(|_| ())
        };
        let changes = self.fetcher.poll(&mut register);
        if changes.is_empty() {
            return false;
        }
        let mut downloaded = false;
        for (family, status) in &changes {
            downloaded |= *status == FamilyStatus::Downloaded;
            host.editor_state_mut()
                .editor_ui
                .font_downloads
                .set_status(family, ui_status(status));
        }
        if downloaded {
            host.refresh_imported_fonts();
            host.refresh_missing_fonts_prompt();
        }
        host.mark_editor_state_dirty();
        true
    }

    /// A row that still shows although its family was "downloaded" did not
    /// resolve (the user removed the font again, or the OS reports another
    /// family name): offer Retry with a plain "Download failed" instead of a
    /// dead "Downloaded". Runs every frame; it only walks the few rows.
    fn demote_unresolved_downloads(&mut self, host: &mut WidgetHostNative) -> bool {
        let ui = &mut host.editor_state_mut().editor_ui;
        let Some(prompt) = ui.missing_fonts_prompt.as_ref() else {
            return false;
        };
        let stale: Vec<String> = prompt
            .entries
            .iter()
            .filter(|entry| {
                !entry.resolved
                    && ui.font_downloads.status(&entry.family)
                        == Some(FontDownloadStatus::Downloaded)
            })
            .map(|entry| entry.family.clone())
            .collect();
        for family in &stale {
            ui.font_downloads.set_status(
                family,
                FontDownloadStatus::Failed(FontDownloadFailure::Other),
            );
        }
        !stale.is_empty()
    }
}

#[cfg(test)]
pub(crate) fn font_registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[path = "font_download_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "font_download_weight_tests.rs"]
mod weight_tests;
