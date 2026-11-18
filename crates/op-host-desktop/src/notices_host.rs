//! File ▸ Third-party notices: open the `THIRD_PARTY_NOTICES.md` that
//! `scripts/stage-notices.sh` staged beside the program, with the system
//! opener. When the build has no staged copy (a `cargo run` from a checkout,
//! an unusual install) the repository's copy of the same file is opened in the
//! browser instead, so the item always shows the user something.

use std::path::{Path, PathBuf};

const FILE_NAME: &str = "THIRD_PARTY_NOTICES.md";
const REPOSITORY_FILE_URL: &str =
    "https://github.com/baroned1707/openpencil/blob/main/packaging/shared/THIRD_PARTY_NOTICES.md";

/// Where each package layout puts the staged notices, relative to the
/// directory that holds the executable: release archives and the Windows
/// installer (`notices` next to the program), the macOS bundle
/// (`Contents/MacOS` -> `Contents/Resources/notices`) and the Linux packages
/// (`usr/bin` -> `usr/share/doc/openpencil/notices`).
const CANDIDATE_DIRS: [&str; 3] = [
    "notices",
    "../Resources/notices",
    "../share/doc/openpencil/notices",
];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NoticesTarget {
    /// The staged file on disk.
    File(PathBuf),
    /// The repository's copy, used when no staged file exists.
    Url(&'static str),
}

/// Pick what to open. Pure: `exists` answers whether a path is a file, so the
/// layouts can be tested without touching the disk.
pub(crate) fn resolve_notices_target(
    executable: &Path,
    exists: impl Fn(&Path) -> bool,
) -> NoticesTarget {
    let Some(directory) = executable.parent() else {
        return NoticesTarget::Url(REPOSITORY_FILE_URL);
    };
    for relative in CANDIDATE_DIRS {
        let mut candidate = directory.to_path_buf();
        // Resolve `..` lexically: the Windows launcher dislikes it in a path.
        for part in relative.split('/') {
            if part == ".." {
                candidate.pop();
            } else {
                candidate.push(part);
            }
        }
        candidate.push(FILE_NAME);
        if exists(&candidate) {
            return NoticesTarget::File(candidate);
        }
    }
    NoticesTarget::Url(REPOSITORY_FILE_URL)
}

/// Open the notices. Best effort like every other opener: a missing launcher
/// is logged by `open_url`.
pub(crate) fn open_third_party_notices() {
    let target = match std::env::current_exe() {
        Ok(executable) => resolve_notices_target(&executable, Path::is_file),
        Err(_) => NoticesTarget::Url(REPOSITORY_FILE_URL),
    };
    match target {
        NoticesTarget::File(path) => crate::update_check::open_url(&path.to_string_lossy()),
        NoticesTarget::Url(url) => crate::update_check::open_url(url),
    }
}

#[cfg(test)]
#[path = "notices_host_tests.rs"]
mod tests;
