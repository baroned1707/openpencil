//! What a file looked like on disk when the editor last read or wrote it.
//!
//! A document saved "in place" by an agent (or by autosave) must never replace
//! a file somebody else changed since the editor opened it. The editor
//! remembers a [`DiskFingerprint`] of each open document's file and compares it
//! with the file's current one right before a write. Size + modification time
//! (+ inode on Unix, which an external editor's rename-over changes even when
//! the other two match) is cheap; hashing a large `.op` on every save is not.

use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// One observation of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFingerprint {
    /// The path did not exist.
    Missing,
    Present {
        len: u64,
        modified_nanos: u128,
        /// File identity on Unix; `0` where the platform has none.
        inode: u64,
    },
}

impl DiskFingerprint {
    /// Observe `path`. A missing file is a normal observation; any other
    /// failure to stat it is an error, so a caller never treats "could not
    /// look" as "unchanged".
    pub fn capture(path: &Path) -> io::Result<Self> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::Missing),
            Err(error) => return Err(error),
        };
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_nanos());
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(&metadata);
        #[cfg(not(unix))]
        let inode = 0;
        Ok(Self::Present {
            len: metadata.len(),
            modified_nanos,
            inode,
        })
    }
}

/// Why a file may not be overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskCheckError {
    /// The file is not what the editor last saw.
    Changed,
    /// The file could not be examined.
    Unreadable(String),
}

/// `Ok` only when `path` still looks exactly like `baseline`.
pub fn check_unchanged(baseline: &DiskFingerprint, path: &Path) -> Result<(), DiskCheckError> {
    match DiskFingerprint::capture(path) {
        Ok(current) if current == *baseline => Ok(()),
        Ok(_) => Err(DiskCheckError::Changed),
        Err(error) => Err(DiskCheckError::Unreadable(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-disk-fingerprint-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn an_untouched_file_matches_and_a_missing_file_is_an_observation() {
        let dir = scratch("same");
        let file = dir.join("a.op");
        assert_eq!(
            DiskFingerprint::capture(&file).unwrap(),
            DiskFingerprint::Missing
        );
        std::fs::write(&file, b"one").unwrap();
        let baseline = DiskFingerprint::capture(&file).unwrap();
        assert_eq!(check_unchanged(&baseline, &file), Ok(()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_rewrite_a_delete_and_a_creation_are_all_changes() {
        let dir = scratch("changed");
        let file = dir.join("a.op");
        std::fs::write(&file, b"one").unwrap();
        let baseline = DiskFingerprint::capture(&file).unwrap();
        // Same length, so only the modification time or inode can tell.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, b"two").unwrap();
        assert_eq!(
            check_unchanged(&baseline, &file),
            Err(DiskCheckError::Changed)
        );
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            check_unchanged(&baseline, &file),
            Err(DiskCheckError::Changed)
        );
        assert_eq!(
            check_unchanged(&DiskFingerprint::Missing, &file),
            Ok(()),
            "a file that was missing and still is, is unchanged"
        );
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            check_unchanged(&DiskFingerprint::Missing, &file),
            Err(DiskCheckError::Changed)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
