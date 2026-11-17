//! Tests for the downloader core. Nothing here uses the network: every
//! request goes to a [`crate::fake::FakeFontSource`].

mod catalog_tests;
mod fetch_tests;
mod metadata_tests;
mod session_tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A unique scratch directory removed on drop.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("op-font-fetch-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
