//! Download one family from google/fonts: find its directory, read and
//! cross-check `METADATA.pb`, plan the files, fetch them and the licence text.
//! Pure over [`FontSource`]; no skia, no disk, no threads.

use std::collections::BTreeSet;
use std::fmt;
use std::time::Instant;

use crate::catalog::{normalize_family, Catalog, CatalogEntry};
use crate::license::License;
use crate::metadata::{parse_metadata, MAX_METADATA_BYTES};
use crate::plan::{plan_files, FaceRequest};
use crate::source::{FetchError, FontSource};
use crate::urls::{directory_token, font_file_url, license_url, metadata_url};

const MAX_LICENSE_BYTES: usize = 256 * 1024;

/// Why a family was not downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchFailure {
    /// Not in the Google part of the catalog (proprietary or invented name).
    UnknownFont,
    /// In the catalog, but not under an allow-listed licence (or the two
    /// licence sources disagree).
    LicenseNotAllowed,
    Offline,
    Timeout,
    /// Catalogued, but no directory in google/fonts matches.
    NotFound,
    TooLarge,
    /// The bytes or metadata were not what the format requires.
    Invalid(String),
    /// The per-session download cap was reached.
    BudgetExceeded,
    Http(u16),
    Other(String),
}

impl FetchFailure {
    /// Whether the user clicking Download again can plausibly help.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Offline | Self::Timeout | Self::Invalid(_) | Self::Http(_) | Self::Other(_)
        )
    }
}

impl fmt::Display for FetchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFont => f.write_str("not a Google Fonts family"),
            Self::LicenseNotAllowed => f.write_str("licence not allowed"),
            Self::Offline => f.write_str("offline"),
            Self::Timeout => f.write_str("timed out"),
            Self::NotFound => f.write_str("not found in google/fonts"),
            Self::TooLarge => f.write_str("font file too large"),
            Self::Invalid(detail) => write!(f, "invalid font data: {detail}"),
            Self::BudgetExceeded => f.write_str("session download limit reached"),
            Self::Http(code) => write!(f, "HTTP status {code}"),
            Self::Other(detail) => f.write_str(detail),
        }
    }
}

impl From<FetchError> for FetchFailure {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::NotFound => Self::NotFound,
            FetchError::Offline => Self::Offline,
            FetchError::Timeout => Self::Timeout,
            FetchError::TooLarge => Self::TooLarge,
            FetchError::Status(code) => Self::Http(code),
            FetchError::Other(detail) => Self::Other(detail),
        }
    }
}

/// Limits for one family download.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    pub max_file_bytes: usize,
    /// Whole-family deadline; checked before every request.
    pub deadline: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedFile {
    pub file_name: String,
    pub italic: bool,
    pub weight: u16,
    pub url: String,
    pub bytes: Vec<u8>,
}

/// Everything one family needs to be stored and registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyDownload {
    pub catalog_id: String,
    pub family: String,
    pub license: License,
    pub license_file_name: &'static str,
    pub license_text: Vec<u8>,
    pub metadata_url: String,
    pub files: Vec<DownloadedFile>,
}

impl FamilyDownload {
    pub fn total_bytes(&self) -> usize {
        self.license_text.len() + self.files.iter().map(|f| f.bytes.len()).sum::<usize>()
    }
}

/// Decide, from the catalog alone, whether a family may be fetched.
pub fn classify<'a>(catalog: &'a Catalog, family: &str) -> Result<&'a CatalogEntry, FetchFailure> {
    let entry = catalog.lookup(family).ok_or(FetchFailure::UnknownFont)?;
    entry.license.ok_or(FetchFailure::LicenseNotAllowed)?;
    Ok(entry)
}

fn check_deadline(limits: &FetchLimits) -> Result<(), FetchFailure> {
    if Instant::now() >= limits.deadline {
        Err(FetchFailure::Timeout)
    } else {
        Ok(())
    }
}

/// TrueType outlines only: the `sfnt` version `0x00010000` or `true`. WOFF2,
/// OpenType-CFF, collections and HTML error pages are all refused.
pub fn looks_like_truetype(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x00, 0x01, 0x00, 0x00]) || bytes.starts_with(b"true")
}

/// Download `entry` (which must have passed [`classify`]).
pub fn fetch_family(
    source: &dyn FontSource,
    entry: &CatalogEntry,
    wanted: &BTreeSet<FaceRequest>,
    limits: &FetchLimits,
) -> Result<FamilyDownload, FetchFailure> {
    let catalog_license = entry.license.ok_or(FetchFailure::LicenseNotAllowed)?;
    let directory = directory_token(&entry.id)
        .ok_or_else(|| FetchFailure::Invalid("unusable catalog id".into()))?;
    let wanted_name = normalize_family(&entry.family);

    // Probe ofl, apache, ufl in order; only a 404 (or a directory that holds
    // another family) moves on. A network error must not look like "absent".
    let mut found = None;
    for dir_license in License::PROBE_ORDER {
        check_deadline(limits)?;
        let url = metadata_url(dir_license, &directory);
        match source.get(&url, MAX_METADATA_BYTES) {
            Ok(response) => {
                let text = String::from_utf8(response.bytes)
                    .map_err(|_| FetchFailure::Invalid("METADATA.pb is not UTF-8".into()))?;
                let meta = parse_metadata(&text);
                if meta.name.as_deref().and_then(normalize_family) == wanted_name {
                    found = Some((dir_license, url, meta));
                    break;
                }
            }
            Err(FetchError::NotFound) => {}
            Err(other) => return Err(other.into()),
        }
    }
    let (dir_license, metadata_url, meta) = found.ok_or(FetchFailure::NotFound)?;

    // Both licence sources must agree and be allowed.
    let meta_license = meta
        .license
        .as_deref()
        .and_then(License::from_metadata)
        .ok_or(FetchFailure::LicenseNotAllowed)?;
    if meta_license != catalog_license {
        return Err(FetchFailure::LicenseNotAllowed);
    }

    let planned = plan_files(&meta, wanted)
        .ok_or_else(|| FetchFailure::Invalid("no downloadable .ttf listed".into()))?;

    let mut files = Vec::with_capacity(planned.len());
    for file in planned {
        check_deadline(limits)?;
        let url = font_file_url(dir_license, &directory, &file.file_name);
        let response = source.get(&url, limits.max_file_bytes)?;
        if response.bytes.len() > limits.max_file_bytes {
            return Err(FetchFailure::TooLarge);
        }
        if !looks_like_truetype(&response.bytes) {
            return Err(FetchFailure::Invalid(format!(
                "{} is not a TrueType font",
                file.file_name
            )));
        }
        files.push(DownloadedFile {
            file_name: file.file_name,
            italic: file.italic,
            weight: file.weight,
            url,
            bytes: response.bytes,
        });
    }

    check_deadline(limits)?;
    let license_response = source
        .get(&license_url(dir_license, &directory), MAX_LICENSE_BYTES)
        .map_err(|error| match error {
            FetchError::NotFound => FetchFailure::Invalid("licence text missing".into()),
            other => other.into(),
        })?;

    Ok(FamilyDownload {
        catalog_id: entry.id.clone(),
        family: entry.family.clone(),
        license: meta_license,
        license_file_name: meta_license.file_name(),
        license_text: license_response.bytes,
        metadata_url,
        files,
    })
}
