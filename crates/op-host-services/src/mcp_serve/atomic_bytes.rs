//! Crash-safe file write for the MCP tools that put bytes at a caller-named
//! path (`save_document`).
//!
//! A plain `fs::write` truncates the target first, so a crash or a full disk
//! mid-write leaves a half-written file where a good one used to be. This
//! writes a sibling temp file, flushes it to disk, then renames it over the
//! target, using the same primitives as the editor's own `.op` save
//! (`doc_io::atomic_file`), so there is one commit path to reason about.

use std::io::Write;
use std::path::Path;

use crate::doc_io::atomic_file::{create_sibling_temp, replace_file_keeping_previous};

/// Replace `path` with `bytes` atomically. Never leaves the temp file behind.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let (temp, mut file) = create_sibling_temp(path).map_err(into_io)?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    let committed = match written {
        Ok(()) => replace_file_keeping_previous(&temp, path).map_err(into_io),
        Err(error) => Err(error),
    };
    if committed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    committed
}

fn into_io(error: crate::doc_io::DocIoError) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::write_atomically;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-atomic-bytes-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn replaces_the_target_and_leaves_no_temp_file() {
        let dir = scratch("replace");
        let target = dir.join("design.op");
        std::fs::write(&target, b"old").expect("seed");
        write_atomically(&target, b"new content").expect("write");
        assert_eq!(std::fs::read(&target).expect("read"), b"new content");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("design.op")]);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file_and_cleans_up() {
        let dir = scratch("failed");
        let target = dir.join("design.op");
        std::fs::write(&target, b"old").expect("seed");
        // Renaming a file over a directory fails: the commit step errors.
        let blocked = dir.join("blocked");
        std::fs::create_dir(&blocked).expect("dir");
        assert!(write_atomically(&blocked, b"x").is_err());
        assert_eq!(std::fs::read(&target).expect("read"), b"old");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .expect("list")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            names.iter().all(|name| !name.ends_with(".tmp")),
            "{names:?}"
        );
    }

    #[test]
    fn the_headless_and_mcp_byte_writer_keeps_the_replaced_bytes() {
        use crate::doc_io::version_backup::{
            enable_version_history, versions_dir_for_document, with_origin, WrittenBy,
        };
        enable_version_history();
        let dir = scratch("history");
        let target = dir.join("design.op");
        std::fs::write(&target, b"before").expect("seed");
        with_origin(WrittenBy::Mcp, || write_atomically(&target, b"after")).expect("write");
        assert_eq!(std::fs::read(&target).expect("read"), b"after");
        let folder = versions_dir_for_document(&target).expect("settings dir");
        let kept: Vec<_> = std::fs::read_dir(&folder)
            .expect("history folder")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".op"))
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read(kept[0].path()).expect("version"), b"before");
    }
}
