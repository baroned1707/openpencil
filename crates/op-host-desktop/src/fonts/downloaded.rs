//! Storing a family downloaded by `op-font-fetch`: the licence text and a
//! small provenance record beside the fonts, then each face through the normal
//! [`FontStore::import`] (persist, then register), so a restart restores the
//! family through `rescan_and_register` exactly like a hand-imported one.
//!
//! Layout under `<config>/fonts/`:
//! `licenses/<catalog-id>/<OFL.txt | LICENSE.txt | UFL.txt>` and
//! `licenses/<catalog-id>/source.json`. `<catalog-id>` is the validated
//! Fontsource id (`^[a-z0-9-]+$`), never the document's family string.

use std::time::{SystemTime, UNIX_EPOCH};

use op_font_fetch::catalog::{is_valid_catalog_id, normalize_family};
use op_font_fetch::{FamilyDownload, FetchFailure};

use super::{FontStore, MAX_FONT_BYTES};

const LICENSES_SUBDIR: &str = "licenses";

impl FontStore {
    /// Validate every file, write the licence + `source.json`, then import the
    /// faces. Nothing touches disk or the live registry unless ALL files are
    /// well-formed fonts of the expected family, so a bad download is simply
    /// not registered.
    pub(crate) fn store_downloaded_family(
        &self,
        download: &FamilyDownload,
    ) -> Result<Vec<jian_skia::FontBlob>, FetchFailure> {
        if !is_valid_catalog_id(&download.catalog_id) {
            return Err(FetchFailure::Invalid("unusable catalog id".into()));
        }
        let expected = normalize_family(&download.family);
        for file in &download.files {
            if file.bytes.len() > MAX_FONT_BYTES {
                return Err(FetchFailure::TooLarge);
            }
            let meta = jian_skia::parse_imported_font_meta(&file.bytes).ok_or_else(|| {
                FetchFailure::Invalid(format!("{} is not a font skia can read", file.file_name))
            })?;
            if normalize_family(&meta.family) != expected {
                return Err(FetchFailure::Invalid(format!(
                    "{} declares the family {:?}, not {:?}",
                    file.file_name, meta.family, download.family
                )));
            }
        }

        self.write_license_files(download)?;

        let mut blobs = Vec::with_capacity(download.files.len());
        for file in &download.files {
            let blob = self
                .import(file.bytes.clone())
                .map_err(|error| FetchFailure::Invalid(error.to_string()))?;
            blobs.push(blob);
        }
        Ok(blobs)
    }

    fn write_license_files(&self, download: &FamilyDownload) -> Result<(), FetchFailure> {
        let dir = self.dir.join(LICENSES_SUBDIR).join(&download.catalog_id);
        let io = |error: std::io::Error| FetchFailure::Other(format!("write licence: {error}"));
        std::fs::create_dir_all(&dir).map_err(io)?;
        std::fs::write(dir.join(download.license_file_name), &download.license_text).map_err(io)?;
        let record = serde_json::json!({
            "family": download.family,
            "catalogId": download.catalog_id,
            "license": download.license.label(),
            "metadataUrl": download.metadata_url,
            "files": download.files.iter().map(|file| serde_json::json!({
                "name": file.file_name,
                "url": file.url,
            })).collect::<Vec<_>>(),
            "downloadedAtUnix": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
        });
        let text = serde_json::to_vec_pretty(&record)
            .map_err(|error| FetchFailure::Other(format!("source.json: {error}")))?;
        std::fs::write(dir.join("source.json"), text).map_err(io)
    }

    /// Path of the Fontsource catalog cache.
    pub(crate) fn catalog_cache_path(&self) -> std::path::PathBuf {
        self.dir.join("fontsource-catalog.json")
    }

    #[cfg(test)]
    pub(crate) fn licenses_dir(&self) -> std::path::PathBuf {
        self.dir.join(LICENSES_SUBDIR)
    }
}
