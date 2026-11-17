//! macOS Trash: `NSFileManager trashItemAtURL:resultingItemURL:error:`.
//!
//! The same call Finder's "Move to Trash" makes: the file stays recoverable
//! (Put Back works) and nothing is deleted. It needs no entitlement for a file
//! the user can already write.

use std::io;
use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

/// Move `path` to the user's Trash.
pub(super) fn move_to_trash(path: &Path) -> io::Result<()> {
    let text = path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "the path is not valid UTF-8")
    })?;
    // SAFETY: `defaultManager` and `fileURLWithPath:` are plain Foundation
    // factory calls; `trashItemAtURL:resultingItemURL:error:` is passed a live
    // URL and no out pointer. Its failure comes back as an `NSError`.
    unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(text));
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|error| io::Error::other(error.localizedDescription().to_string()))
    }
}
