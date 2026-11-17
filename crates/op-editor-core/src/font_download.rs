//! UI-side state of the missing-font downloader.
//!
//! This module is platform-free (the web bundle compiles it, but only the
//! desktop host ever sets [`FontDownloadUi::supported`] or fills the status
//! map). It holds what the missing-font surfaces need to draw: the per-family
//! status, the failure reason, and the requests the user raised by clicking
//! Download / Download all, which the desktop host drains each frame.

use std::collections::BTreeMap;

use crate::missing_fonts::MissingFontEntry;

/// Why a family could not be downloaded. Coarse on purpose: each variant maps
/// to one localized one-line reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontDownloadFailure {
    /// Not a Google Fonts family (proprietary or invented name).
    UnknownFont,
    /// Known, but under a licence the editor does not download.
    LicenseNotAllowed,
    Offline,
    Timeout,
    /// Catalogued but absent from the files repository.
    NotFound,
    TooLarge,
    /// The download was not a usable font.
    Invalid,
    /// The per-session download limit was reached.
    BudgetExceeded,
    Other,
}

impl FontDownloadFailure {
    /// Whether clicking Download again can plausibly succeed.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Offline | Self::Timeout | Self::Invalid | Self::Other
        )
    }

    /// i18n key of the one-line reason.
    pub fn reason_key(self) -> &'static str {
        match self {
            Self::UnknownFont => "missingFonts.download.unknown",
            Self::LicenseNotAllowed => "missingFonts.download.license",
            Self::Offline => "missingFonts.download.offline",
            Self::Timeout => "missingFonts.download.timeout",
            Self::NotFound => "missingFonts.download.notFound",
            Self::TooLarge => "missingFonts.download.tooLarge",
            Self::Invalid => "missingFonts.download.invalid",
            Self::BudgetExceeded => "missingFonts.download.limit",
            Self::Other => "missingFonts.download.failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontDownloadStatus {
    /// May be downloaded (or downloadability is not known yet).
    Ready,
    Downloading,
    Downloaded,
    Failed(FontDownloadFailure),
}

/// What a click on Download / Download all asks the host to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontDownloadRequest {
    /// One missing family, spelled as the row shows it.
    Family(String),
    /// Every row that can still be downloaded.
    All,
}

/// The action a row offers, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowDownloadAction {
    Download,
    Retry,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontDownloadUi {
    /// Only the desktop host sets this; everywhere else the actions stay hidden.
    pub supported: bool,
    statuses: BTreeMap<String, FontDownloadStatus>,
    /// Requests raised by a click, drained by the host.
    pub pending: Vec<FontDownloadRequest>,
}

impl FontDownloadUi {
    /// Lookup key: the same lowercasing `detect_missing_fonts` uses for rows.
    pub fn key(family: &str) -> String {
        family.trim().to_lowercase()
    }

    pub fn status(&self, family: &str) -> Option<FontDownloadStatus> {
        self.statuses.get(&Self::key(family)).copied()
    }

    pub fn set_status(&mut self, family: &str, status: FontDownloadStatus) {
        self.statuses.insert(Self::key(family), status);
    }

    pub fn clear_statuses(&mut self) {
        self.statuses.clear();
    }

    pub fn any_downloading(&self) -> bool {
        self.statuses
            .values()
            .any(|status| *status == FontDownloadStatus::Downloading)
    }

    /// The button a still-missing row shows: Download while it may be fetched,
    /// Retry after a failure that can heal, nothing otherwise (downloading,
    /// downloaded, unknown font, licence not allowed, ...).
    pub fn row_action(&self, entry: &MissingFontEntry) -> Option<RowDownloadAction> {
        if !self.supported || entry.resolved {
            return None;
        }
        match self.status(&entry.family) {
            None | Some(FontDownloadStatus::Ready) => Some(RowDownloadAction::Download),
            Some(FontDownloadStatus::Failed(failure)) if failure.retryable() => {
                Some(RowDownloadAction::Retry)
            }
            Some(FontDownloadStatus::Downloading)
            | Some(FontDownloadStatus::Downloaded)
            | Some(FontDownloadStatus::Failed(_)) => None,
        }
    }

    /// How many rows would a Download all start.
    pub fn downloadable_rows(&self, entries: &[MissingFontEntry]) -> usize {
        entries
            .iter()
            .filter(|entry| self.row_action(entry).is_some())
            .count()
    }

    /// Whether any row is currently being fetched.
    pub fn downloading_rows(&self, entries: &[MissingFontEntry]) -> usize {
        entries
            .iter()
            .filter(|entry| {
                !entry.resolved
                    && self.status(&entry.family) == Some(FontDownloadStatus::Downloading)
            })
            .count()
    }
}

#[cfg(test)]
#[path = "font_download_tests.rs"]
mod tests;
