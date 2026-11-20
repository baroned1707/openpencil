//! Keep the bytes a save is about to replace, so an overwrite is never final.
//!
//! Every `.op` writer ends in an atomic rename over the destination. When the
//! destination already exists, its old bytes would be gone for good: a stale
//! tab, a second instance or a `git checkout` that the editor never noticed can
//! all lead to that. Just before the rename the previous bytes are copied into
//! `<settings dir>/history/<hash of the canonical path>/<UTC time>-<size>.op`
//! (never inside the user's project, so no repository pollution), with a small
//! `meta.json` naming the original path and one `<version>.json` naming who
//! replaced it.
//!
//! A backup is best effort: [`backup_before_replace`] logs a failure and
//! returns, it never blocks the user's save. Retention is bounded per file
//! (newest 20) and across all files (512 MiB, oldest first); the newest
//! version of a file is never pruned. Everything is created owner-only
//! (0600 / 0700) and nothing is read or written through a symlink.

use std::cell::Cell;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

/// Versions kept per file.
pub const DEFAULT_MAX_VERSIONS: usize = 20;
/// Bytes kept across every file's versions.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// A file larger than this is not backed up (the log says so).
pub const DEFAULT_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Retention and size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_versions: usize,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_versions: DEFAULT_MAX_VERSIONS,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

/// Which save replaced the file; recorded in the version's sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrittenBy {
    CmdS,
    SaveAs,
    Autosave,
    Mcp,
    Headless,
    /// Any other `.op` writer (import publish, legacy upgrade, a CLI).
    Other,
}

impl WrittenBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CmdS => "cmd-s",
            Self::SaveAs => "save-as",
            Self::Autosave => "autosave",
            Self::Mcp => "mcp",
            Self::Headless => "headless",
            Self::Other => "other",
        }
    }
}

/// What [`backup_existing`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupOutcome {
    /// The old bytes are now this version file.
    Created(PathBuf),
    /// The destination does not exist yet: nothing to keep.
    SkippedMissing,
    /// The newest version already holds exactly these bytes.
    SkippedIdentical,
    /// The destination is above the size limit.
    SkippedTooLarge { bytes: u64 },
    /// A symlink or other non-regular entry: nothing is read through it. A
    /// rename over a symlink replaces the link, not the file it points to.
    SkippedNotRegular,
}

/// Why a backup could not be made.
#[derive(Debug)]
pub enum BackupError {
    Io {
        step: &'static str,
        source: io::Error,
    },
    /// A history directory is a symlink or not a directory.
    UnsafeDirectory(PathBuf),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { step, source } => write!(f, "{step}: {source}"),
            Self::UnsafeDirectory(path) => write!(
                f,
                "{} is not a plain directory; refusing to use it",
                path.display()
            ),
        }
    }
}

impl std::error::Error for BackupError {}

fn io_step(step: &'static str) -> impl FnOnce(io::Error) -> BackupError {
    move |source| BackupError::Io { step, source }
}

/// Serializes backup + prune, so two saves never prune under each other.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// The folder holding every file's versions, inside `settings_dir`.
pub fn history_root(settings_dir: &Path) -> PathBuf {
    settings_dir.join("history")
}

/// Stable identity of a document path: the canonical path when it exists, else
/// the canonical parent plus the file name, else the path as given.
fn canonical_identity(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            std::fs::canonicalize(parent).map_or_else(|_| path.to_path_buf(), |dir| dir.join(name))
        }
        _ => path.to_path_buf(),
    }
}

/// The folder that holds the versions of `document` under `history_root`
/// (SHA-256 of the canonical path, first 32 hex digits: stable across builds).
pub fn versions_dir_in(history_root: &Path, document: &Path) -> PathBuf {
    let identity = canonical_identity(document);
    let digest = Sha256::digest(identity.to_string_lossy().as_bytes());
    let name: String = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    history_root.join(name)
}

/// [`versions_dir_in`] for the user's settings directory.
pub fn versions_dir_for_document(document: &Path) -> Option<PathBuf> {
    let settings = crate::settings_io::settings_dir()?;
    Some(versions_dir_in(&history_root(&settings), document))
}

/// Create `dir` (one level) owner-only, or accept an existing real directory.
fn ensure_private_dir(dir: &Path) -> Result<(), BackupError> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => Err(BackupError::UnsafeDirectory(dir.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = dir.parent() {
                std::fs::create_dir_all(parent).map_err(io_step("create history parent"))?;
            }
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
            match builder.create(dir) {
                Ok(()) => Ok(()),
                // Lost a race with another process: fine if it is a real dir.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    match std::fs::symlink_metadata(dir) {
                        Ok(meta) if meta.file_type().is_dir() => Ok(()),
                        _ => Err(BackupError::UnsafeDirectory(dir.to_path_buf())),
                    }
                }
                Err(error) => Err(BackupError::Io {
                    step: "create history directory",
                    source: error,
                }),
            }
        }
        Err(error) => Err(BackupError::Io {
            step: "inspect history directory",
            source: error,
        }),
    }
}

fn create_private_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// `YYYYMMDDTHHMMSSmmmZ` for `unix_millis` (UTC), so names sort by time.
fn utc_stamp(unix_millis: u128) -> String {
    let millis = (unix_millis % 1000) as u32;
    let total_seconds = (unix_millis / 1000) as i64;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}{millis:03}Z",
        seconds_of_day / 3600,
        seconds_of_day % 3600 / 60,
        seconds_of_day % 60
    )
}

/// One stored version.
#[derive(Debug, Clone)]
struct Version {
    name: String,
    bytes: u64,
}

fn is_version_name(name: &str) -> bool {
    !name.starts_with('.') && name.ends_with(".op")
}

fn sidecar_of(version_path: &Path) -> PathBuf {
    version_path.with_extension("json")
}

/// The versions in `dir`, oldest first (names sort by time).
fn list_versions(dir: &Path) -> Vec<Version> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let meta = entry.metadata().ok()?;
            (is_version_name(&name) && meta.is_file()).then_some(Version {
                name,
                bytes: meta.len(),
            })
        })
        .collect();
    versions.sort_by(|a, b| a.name.cmp(&b.name));
    versions
}

fn same_bytes(left: &Path, right: &Path) -> io::Result<bool> {
    let mut left = std::fs::File::open(left)?;
    let mut right = std::fs::File::open(right)?;
    if left.metadata()?.len() != right.metadata()?.len() {
        return Ok(false);
    }
    let mut left_buffer = vec![0u8; 64 * 1024];
    let mut right_buffer = vec![0u8; 64 * 1024];
    loop {
        let read = left.read(&mut left_buffer)?;
        if read == 0 {
            return Ok(true);
        }
        right.read_exact(&mut right_buffer[..read])?;
        if left_buffer[..read] != right_buffer[..read] {
            return Ok(false);
        }
    }
}

fn write_text_file(path: &Path, text: &str) -> io::Result<()> {
    let mut file = create_private_file(path)?;
    file.write_all(text.as_bytes())
}

fn replace_text_file(path: &Path, text: &str) -> io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&temporary);
    write_text_file(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

/// Keep the current bytes of `destination` as a new version under
/// `history_root`, unless there is nothing to keep (see [`BackupOutcome`]).
/// Prunes afterwards. The caller treats an `Err` as "log and carry on".
pub fn backup_existing(
    history_root: &Path,
    destination: &Path,
    written_by: WrittenBy,
    limits: &Limits,
) -> Result<BackupOutcome, BackupError> {
    let _guard = STORE_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let metadata = match std::fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(BackupOutcome::SkippedMissing)
        }
        Err(error) => return Err(io_step("inspect destination")(error)),
    };
    if !metadata.file_type().is_file() {
        return Ok(BackupOutcome::SkippedNotRegular);
    }
    if metadata.len() > limits.max_file_bytes {
        return Ok(BackupOutcome::SkippedTooLarge {
            bytes: metadata.len(),
        });
    }
    ensure_private_dir(history_root)?;
    let dir = versions_dir_in(history_root, destination);
    ensure_private_dir(&dir)?;

    let existing = list_versions(&dir);
    if let Some(newest) = existing.last() {
        if same_bytes(&dir.join(&newest.name), destination).unwrap_or(false) {
            return Ok(BackupOutcome::SkippedIdentical);
        }
    }

    let source = std::fs::File::open(destination).map_err(io_step("open destination"))?;
    let temporary = dir.join(format!(".incoming-{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    let copied = (|| -> io::Result<u64> {
        let mut target = create_private_file(&temporary)?;
        let bytes = io::copy(&mut io::BufReader::new(source), &mut target)?;
        target.sync_all()?;
        Ok(bytes)
    })();
    let bytes = match copied {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(io_step("copy previous bytes")(error));
        }
    };

    let now_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let mut offset = 0u128;
    let final_path = loop {
        let name = format!("{}-{bytes}.op", utc_stamp(now_millis + offset));
        let candidate = dir.join(name);
        if std::fs::symlink_metadata(&candidate).is_err() {
            break candidate;
        }
        offset += 1;
    };
    if let Err(error) = std::fs::rename(&temporary, &final_path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(io_step("publish version")(error));
    }

    // Sidecars are informative: a failure to write one never loses the version.
    let original = destination.to_string_lossy();
    let meta = serde_json::json!({ "originalPath": original });
    let _ = replace_text_file(&dir.join("meta.json"), &meta.to_string());
    let sidecar = serde_json::json!({
        "originalPath": original,
        "writtenBy": written_by.as_str(),
        "bytes": bytes,
        "backedUpAtUnixMillis": now_millis as u64,
    });
    let _ = write_text_file(&sidecar_of(&final_path), &sidecar.to_string());

    prune(history_root, &dir, limits);
    Ok(BackupOutcome::Created(final_path))
}

fn remove_version(dir: &Path, version: &Version) {
    let path = dir.join(&version.name);
    let _ = std::fs::remove_file(sidecar_of(&path));
    let _ = std::fs::remove_file(path);
}

/// Enforce the per-file count in `own_dir`, then the total size across every
/// file's folder. Oldest goes first; the newest version of each file stays.
fn prune(history_root: &Path, own_dir: &Path, limits: &Limits) {
    let own = list_versions(own_dir);
    let keep = limits.max_versions.max(1);
    if own.len() > keep {
        for version in &own[..own.len() - keep] {
            remove_version(own_dir, version);
        }
    }

    let Ok(folders) = std::fs::read_dir(history_root) else {
        return;
    };
    // (folder, version, is the newest of its folder)
    let mut all: Vec<(PathBuf, Version, bool)> = Vec::new();
    for folder in folders.flatten() {
        let Ok(kind) = folder.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let versions = list_versions(&folder.path());
        let count = versions.len();
        for (index, version) in versions.into_iter().enumerate() {
            all.push((folder.path(), version, index + 1 == count));
        }
    }
    let mut total: u64 = all.iter().map(|(_, version, _)| version.bytes).sum();
    if total <= limits.max_total_bytes {
        return;
    }
    all.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    for (folder, version, newest_of_folder) in &all {
        if total <= limits.max_total_bytes {
            break;
        }
        if *newest_of_folder {
            continue;
        }
        remove_version(folder, version);
        total = total.saturating_sub(version.bytes);
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Turn on version backups for this process. Only hosts that own a user's
/// documents call it (the desktop and the headless `--mcp` server); the web
/// daemon never does, so tenant documents never share one history.
pub fn enable_version_history() {
    ENABLED.store(true, Ordering::SeqCst);
}

pub fn version_history_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

thread_local! {
    static ORIGIN: Cell<Option<WrittenBy>> = const { Cell::new(None) };
}

/// Restores the previous origin on drop.
struct OriginGuard(Option<WrittenBy>);

impl Drop for OriginGuard {
    fn drop(&mut self) {
        ORIGIN.with(|cell| cell.set(self.0));
    }
}

/// Run `f` with every backup on this thread attributed to `by`.
pub fn with_origin<T>(by: WrittenBy, f: impl FnOnce() -> T) -> T {
    let _guard = OriginGuard(ORIGIN.with(|cell| cell.replace(Some(by))));
    f()
}

/// [`with_origin`] that leaves an origin an outer caller already chose alone.
pub fn with_default_origin<T>(by: WrittenBy, f: impl FnOnce() -> T) -> T {
    if ORIGIN.with(Cell::get).is_some() {
        f()
    } else {
        with_origin(by, f)
    }
}

/// Called by every writer right before it replaces `destination`. Does
/// nothing unless [`enable_version_history`] was called; never fails the save.
pub fn backup_before_replace(destination: &Path) {
    if !version_history_enabled() {
        return;
    }
    let Some(settings) = crate::settings_io::settings_dir() else {
        return;
    };
    // The app's own state (recovery snapshots, templates, this history) is not
    // a user document.
    if canonical_identity(destination).starts_with(canonical_identity(&settings)) {
        return;
    }
    let written_by = ORIGIN.with(Cell::get).unwrap_or(WrittenBy::Other);
    match backup_existing(
        &history_root(&settings),
        destination,
        written_by,
        &Limits::default(),
    ) {
        Ok(BackupOutcome::Created(version)) => eprintln!(
            "[history] kept the previous {} as {}",
            destination.display(),
            version.display()
        ),
        Ok(BackupOutcome::SkippedTooLarge { bytes }) => eprintln!(
            "[history] {} is {bytes} bytes, above the backup limit; saving without a backup",
            destination.display()
        ),
        Ok(_) => {}
        Err(error) => eprintln!(
            "[history] backup of {} failed ({error}); saving anyway",
            destination.display()
        ),
    }
}

#[cfg(test)]
#[path = "version_backup_tests.rs"]
mod tests;
