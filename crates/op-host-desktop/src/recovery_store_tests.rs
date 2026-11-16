//! The recovery store on its own: atomic one-entry-per-document writes, the
//! age and size bounds, the scan's filtering, and the ordering guard between a
//! clear and a write still in flight. Every test uses its own temp directory;
//! none touches the user's config.

use super::*;
use op_editor_core::EditorState;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-recovery-store-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn store_in(dir: &Path, launch: &str, pid: u32) -> RecoveryStore {
    RecoveryStore::new(dir.to_path_buf(), launch.to_string(), pid)
}

fn info(label: &str, reason: SnapshotReason) -> SnapshotInfo {
    SnapshotInfo {
        original_path: None,
        label: label.to_string(),
        reason,
    }
}

fn snapshot_named(name: &str) -> CanonicalSaveSnapshot {
    let mut state = EditorState::starter();
    state.doc.name = Some(name.to_string());
    CanonicalSaveSnapshot::capture(&state)
}

fn write_entry(store: &RecoveryStore, document: u64, name: &str) -> WriteOutcome {
    store
        .write(
            store.ticket(document),
            &snapshot_named(name),
            &info(name, SnapshotReason::Autosnapshot),
        )
        .expect("write")
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .flatten()
        .map(|item| item.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn document_name(entry: &RecoveryEntry) -> Option<String> {
    let (state, _) = op_host_native::widget_host::capture_loaded_thumbnails(|| {
        op_host_services::doc_io::load_editor_state(
            &entry.document_path,
            op_editor_core::Locale::EnUs,
        )
    });
    state.expect("snapshot loads").doc.name
}

fn nobody_is_alive(_pid: u32) -> bool {
    false
}

#[test]
fn snapshot_is_atomic_and_overwrites_one_entry_per_document() {
    let dir = temp_dir("atomic");
    let store = store_in(&dir, "L1", 4242);

    assert_eq!(write_entry(&store, 7, "first"), WriteOutcome::Committed);
    assert_eq!(write_entry(&store, 7, "second"), WriteOutcome::Committed);

    // One document, one entry: two files, and no temporary or pending leftovers.
    assert_eq!(file_names(&dir), vec!["L1-7.json", "L1-7.op"]);
    let report = store.scan(nobody_is_alive);
    // Entries of the writing launch are not offered to itself.
    assert!(report.entries.is_empty());
    let reader = store_in(&dir, "L2", 1);
    let entries = reader.scan(nobody_is_alive).entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(document_name(&entries[0]).as_deref(), Some("second"));
    assert_eq!(entries[0].manifest.label, "second");
    assert_eq!(entries[0].manifest.document_id, 7);
    assert_eq!(entries[0].manifest.app_pid, 4242);
    assert!(!entries[0].manifest.base_saved);

    // A write that fails half way leaves the previous entry exactly as it was.
    std::fs::create_dir(dir.join("L1-7.op.pending")).expect("block the staging name");
    let failed = store.write(
        store.ticket(7),
        &snapshot_named("third"),
        &info("third", SnapshotReason::Exit),
    );
    assert!(matches!(failed, Err(RecoveryError::Document { .. })));
    let entries = reader.scan(nobody_is_alive).entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(document_name(&entries[0]).as_deref(), Some("second"));
    assert_eq!(entries[0].manifest.reason, SnapshotReason::Autosnapshot);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn manifest_names_the_reason_and_the_original_file() {
    let dir = temp_dir("manifest");
    let store = store_in(&dir, "L1", 99);
    let original = dir.join("design.op");
    store
        .write(
            store.ticket(3),
            &snapshot_named("x"),
            &SnapshotInfo {
                original_path: Some(original.clone()),
                label: "design.op".to_string(),
                reason: SnapshotReason::PreRun,
            },
        )
        .expect("write");

    let text = std::fs::read_to_string(dir.join("L1-3.json")).expect("manifest");
    let json: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(json["reason"], "pre-run");
    assert_eq!(json["original_path"], original.to_string_lossy().as_ref());
    assert_eq!(json["base_saved"], true);
    assert_eq!(json["launch_id"], "L1");
    assert_eq!(json["app_pid"], 99);
    assert!(json["saved_at_unix"].as_u64().is_some_and(|t| t > 0));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_clear_wins_over_a_write_that_was_captured_before_it() {
    let dir = temp_dir("clear-race");
    let store = store_in(&dir, "L1", 1);
    // The snapshot is captured (ticket taken), then the document is saved and
    // cleared, and only then does the slow write get to commit.
    let ticket = store.ticket(5);
    store.clear(5).expect("clear without an entry is fine");

    let outcome = store
        .write(
            ticket,
            &snapshot_named("stale"),
            &info("stale", SnapshotReason::Autosnapshot),
        )
        .expect("write");

    assert_eq!(outcome, WriteOutcome::Superseded);
    assert!(file_names(&dir).is_empty(), "{:?}", file_names(&dir));
    assert!(!store.is_current(&ticket));
    // A snapshot captured after the clear commits normally.
    assert_eq!(write_entry(&store, 5, "fresh"), WriteOutcome::Committed);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_older_snapshot_never_replaces_a_newer_one() {
    let dir = temp_dir("order");
    let store = store_in(&dir, "L1", 1);
    let older = store.ticket(2);
    let newer = store.ticket(2);
    let write = |ticket, name: &str| {
        store
            .write(
                ticket,
                &snapshot_named(name),
                &info(name, SnapshotReason::Exit),
            )
            .expect("write")
    };

    assert_eq!(write(newer, "newer"), WriteOutcome::Committed);
    assert_eq!(write(older, "older"), WriteOutcome::Superseded);

    let entries = store_in(&dir, "L2", 1).scan(nobody_is_alive).entries;
    assert_eq!(document_name(&entries[0]).as_deref(), Some("newer"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn entries_of_a_live_other_instance_are_ignored() {
    let dir = temp_dir("live");
    // Written by a process that is "running": pid 555.
    write_entry(&store_in(&dir, "Lother", 555), 1, "other-instance");
    // Written by a process that is gone: pid 666.
    write_entry(&store_in(&dir, "Lgone", 666), 1, "crashed");
    let reader = store_in(&dir, "Lme", 100);

    let report = reader.scan(|pid| pid == 555);

    let labels: Vec<_> = report
        .entries
        .iter()
        .map(|entry| entry.manifest.label.as_str())
        .collect();
    assert_eq!(labels, vec!["crashed"], "the live instance keeps its entry");
    // The same pid as ours is never "another instance", whatever the probe says.
    let own_pid_entry = temp_dir("live-own");
    write_entry(&store_in(&own_pid_entry, "Lold", 100), 1, "previous-run");
    let report = store_in(&own_pid_entry, "Lme", 100).scan(|_| true);
    assert_eq!(report.entries.len(), 1);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(own_pid_entry);
}

#[cfg(unix)]
#[test]
fn the_real_liveness_probe_tells_a_running_process_from_a_gone_one() {
    use crate::recovery_liveness::process_is_alive;
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .expect("spawn sleep");
    let pid = child.id();
    assert!(process_is_alive(pid), "a running child is alive");
    child.kill().expect("kill");
    child.wait().expect("reap");
    assert!(!process_is_alive(pid), "a reaped child is gone");
    // Ids that would address every process or none must never be probed.
    assert!(!process_is_alive(0));
    assert!(!process_is_alive(u32::MAX));
}

#[test]
fn removing_found_entries_deletes_both_files() {
    let dir = temp_dir("discard");
    write_entry(&store_in(&dir, "L1", 1), 1, "a");
    write_entry(&store_in(&dir, "L1", 1), 2, "b");
    let reader = store_in(&dir, "L2", 2);
    let entries = reader.scan(nobody_is_alive).entries;
    assert_eq!(entries.len(), 2);

    for entry in &entries {
        reader.remove_entry(entry).expect("remove");
    }

    assert!(file_names(&dir).is_empty(), "{:?}", file_names(&dir));
    assert!(reader.scan(nobody_is_alive).entries.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn store_prunes_old_entries_and_respects_the_size_cap() {
    let dir = temp_dir("bounds");
    let writer = store_in(&dir, "L1", 1);
    for document in 1..=3 {
        write_entry(&writer, document, &format!("doc{document}"));
    }
    let reader = store_in(&dir, "L2", 2);

    // Age: far in the future every entry is older than the limit.
    let day = 24 * 60 * 60;
    reader.prune(unix_now() + MAX_AGE_SECS + day);
    assert!(
        reader.scan(nobody_is_alive).entries.is_empty(),
        "entries past 30 days are dropped"
    );

    // Size: a cap that fits two of the three entries drops the oldest first.
    let dir = temp_dir("bounds-size");
    let writer = store_in(&dir, "L1", 1);
    write_entry(&writer, 1, "oldest");
    let one_entry = std::fs::metadata(dir.join("L1-1.op")).expect("size").len()
        + std::fs::metadata(dir.join("L1-1.json"))
            .expect("size")
            .len();
    // Make the manifests' ages distinct, oldest first.
    for (document, name, age) in [(2, "middle", 20), (3, "newest", 10)] {
        write_entry(&writer, document, name);
        let path = dir.join(format!("L1-{document}.json"));
        let mut manifest: RecoveryManifest =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest.saved_at_unix = unix_now() - age;
        std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
    let path = dir.join("L1-1.json");
    let mut manifest: RecoveryManifest =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest.saved_at_unix = unix_now() - 30;
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let capped = store_in(&dir, "L2", 2).with_limits(MAX_AGE_SECS, one_entry * 2 + one_entry / 2);
    capped.prune(unix_now());

    let labels: Vec<_> = capped
        .scan(nobody_is_alive)
        .entries
        .iter()
        .map(|entry| entry.manifest.label.clone())
        .collect();
    assert_eq!(
        labels,
        vec!["middle", "newest"],
        "the oldest entry went first"
    );
    let total: u64 = file_names(&dir)
        .iter()
        .map(|name| std::fs::metadata(dir.join(name)).unwrap().len())
        .sum();
    assert!(total <= one_entry * 2 + one_entry / 2, "total {total}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stale_temporaries_and_orphans_are_pruned_but_fresh_ones_are_not() {
    let dir = temp_dir("orphans");
    std::fs::write(dir.join("L1-4.op"), b"{}").unwrap();
    std::fs::write(dir.join("L1-4.op.pending"), b"{}").unwrap();
    std::fs::write(dir.join(".L1-4.json.1.1.tmp"), b"{}").unwrap();
    std::fs::write(dir.join("notes.txt"), b"keep").unwrap();
    let store = store_in(&dir, "L2", 1);

    // Just written: could be another instance mid-write.
    store.prune(unix_now());
    assert_eq!(file_names(&dir).len(), 4);

    // Long after: gone, but a foreign file is never touched.
    store.prune(unix_now() + 60 * 60);
    assert_eq!(file_names(&dir), vec!["notes.txt"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn corrupt_or_foreign_entries_are_skipped_not_fatal() {
    let dir = temp_dir("corrupt");
    write_entry(&store_in(&dir, "L1", 1), 1, "good");
    // Garbage manifest.
    std::fs::write(dir.join("L1-2.json"), b"{ not json").unwrap();
    std::fs::write(dir.join("L1-2.op"), b"{}").unwrap();
    // A valid manifest whose content names another document than its file.
    let mut foreign: RecoveryManifest =
        serde_json::from_slice(&std::fs::read(dir.join("L1-1.json")).unwrap()).unwrap();
    foreign.document_id = 99;
    std::fs::write(dir.join("L1-3.json"), serde_json::to_vec(&foreign).unwrap()).unwrap();
    std::fs::write(dir.join("L1-3.op"), b"{}").unwrap();
    // A manifest that lost its document.
    std::fs::write(
        dir.join("L1-4.json"),
        std::fs::read(dir.join("L1-1.json")).unwrap(),
    )
    .unwrap();
    // Unrelated files and a directory in the way.
    std::fs::write(dir.join("README.md"), b"hello").unwrap();
    std::fs::create_dir(dir.join("L1-5.json")).unwrap();

    let report = store_in(&dir, "L2", 2).scan(nobody_is_alive);

    assert_eq!(report.entries.len(), 1, "only the good entry survives");
    assert_eq!(report.entries[0].manifest.label, "good");
    assert_eq!(report.skipped, 4);
    // Nothing was deleted by looking.
    assert!(dir.join("L1-2.json").exists() && dir.join("README.md").exists());
    // A directory that does not exist is an empty store, not an error.
    let missing = store_in(&dir.join("nope"), "L2", 2).scan(nobody_is_alive);
    assert!(missing.entries.is_empty() && missing.skipped == 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clearing_a_launch_removes_only_its_files() {
    let dir = temp_dir("launch");
    let mine = store_in(&dir, "L1", 1);
    write_entry(&mine, 1, "a");
    write_entry(&mine, 2, "b");
    write_entry(&store_in(&dir, "L9", 9), 1, "someone else's");

    mine.clear_launch();

    assert_eq!(file_names(&dir), vec!["L9-1.json", "L9-1.op"]);
    let _ = std::fs::remove_dir_all(dir);
}
