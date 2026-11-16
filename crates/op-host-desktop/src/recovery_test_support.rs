//! Fixtures for the crash-recovery tests: a headless `DesktopApp` whose
//! recovery store lives in a temp directory (never the user's config), plus
//! helpers to edit documents and to look at the store's files.

use crate::recovery_host::RecoveryHost;
use crate::recovery_store::RecoveryStore;
use crate::DesktopApp;
use op_editor_core::EditorState;
use op_host_services::doc_io::save_to_path;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// A process id no running process has (valid as a `pid_t`, far above any
/// real pid limit), for the entries of "a previous run that crashed".
pub(crate) const DEAD_PID: u32 = 2_000_000_000;

pub(crate) fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-recovery-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Write the one-frame starter document to `dir/name`.
pub(crate) fn write_document(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    path
}

/// A store for a launch called `launch`, written by process `pid`.
pub(crate) fn store(dir: &Path, launch: &str, pid: u32) -> RecoveryStore {
    RecoveryStore::new(dir.to_path_buf(), launch.to_string(), pid)
}

/// A headless app whose snapshots go to `recovery_dir` as launch `launch`.
/// The periodic interval is zero, so one tick snapshots whatever is unsaved.
pub(crate) fn app_with_recovery(recovery_dir: &Path, launch: &str, pid: u32) -> DesktopApp {
    let mut app = DesktopApp::new(None);
    app.recovery = RecoveryHost::with_store(store(recovery_dir, launch, pid));
    app.recovery.set_interval(Duration::ZERO);
    app
}

/// [`app_with_recovery`] with `names` saved in a work dir and opened as tabs;
/// the last one is on screen. Returns `(app, paths, work_dir, recovery_dir)`.
pub(crate) fn app_with_tabs(
    tag: &str,
    names: &[&str],
) -> (DesktopApp, Vec<PathBuf>, PathBuf, PathBuf) {
    let work = temp_dir(&format!("{tag}-work"));
    let recovery = temp_dir(&format!("{tag}-store"));
    let mut app = app_with_recovery(
        &recovery,
        &format!("Lrun{}", std::process::id()),
        std::process::id(),
    );
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| write_document(&work, name))
        .collect();
    for path in &paths {
        assert!(app.open_document_tab(path.clone()), "{}", path.display());
    }
    (app, paths, work, recovery)
}

/// Make an unsaved edit to the document on screen: a new name, so a snapshot
/// can be told apart from the file it started from.
pub(crate) fn edit_active(app: &mut DesktopApp, name: &str) {
    let state = app.host.editor_state_mut();
    state.doc.name = Some(name.to_string());
    state.mark_document_changed();
}

/// The names of the files in the store directory, sorted.
pub(crate) fn files_in(dir: &Path) -> Vec<String> {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = listing
        .flatten()
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The entries a later launch would find in `dir` (a live-process probe that
/// says nobody is running, so nothing is filtered by pid).
pub(crate) fn entries_in(dir: &Path) -> Vec<crate::recovery_store::RecoveryEntry> {
    store(dir, "Lreader", 1).scan(|_| false).entries
}

/// The document name stored in a snapshot.
pub(crate) fn snapshot_name(entry: &crate::recovery_store::RecoveryEntry) -> Option<String> {
    let (loaded, _) = op_host_native::widget_host::capture_loaded_thumbnails(|| {
        op_host_services::doc_io::load_editor_state(
            &entry.document_path,
            op_editor_core::Locale::EnUs,
        )
    });
    loaded.expect("snapshot loads").doc.name
}
