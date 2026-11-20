use super::*;
use crate::doc_io::atomic_file::create_sibling_temp;

fn scratch(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-version-backup-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn limits() -> Limits {
    Limits::default()
}

fn versions_of(history: &Path, document: &Path) -> Vec<String> {
    list_versions(&versions_dir_in(history, document))
        .into_iter()
        .map(|version| version.name)
        .collect()
}

fn content_of_newest(history: &Path, document: &Path) -> Vec<u8> {
    let dir = versions_dir_in(history, document);
    let newest = list_versions(&dir).pop().expect("a version");
    std::fs::read(dir.join(newest.name)).unwrap()
}

#[test]
fn an_overwrite_keeps_the_previous_bytes_with_a_meta_record() {
    let dir = scratch("keep");
    let history = dir.join("history");
    let document = dir.join("a.op");
    std::fs::write(&document, b"version one").unwrap();

    let outcome = backup_existing(&history, &document, WrittenBy::SaveAs, &limits()).unwrap();
    let BackupOutcome::Created(version) = outcome else {
        panic!("expected a version, got {outcome:?}");
    };
    assert_eq!(std::fs::read(&version).unwrap(), b"version one");
    assert!(version
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with("-11.op"));
    let folder = versions_dir_in(&history, &document);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["originalPath"], document.to_string_lossy().as_ref());
    let sidecar: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sidecar_of(&version)).unwrap()).unwrap();
    assert_eq!(sidecar["writtenBy"], "save-as");
    assert_eq!(sidecar["bytes"], 11);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn history_files_and_folders_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("perms");
    let history = dir.join("history");
    let document = dir.join("a.op");
    std::fs::write(&document, b"secret").unwrap();
    // A world-readable source must not leak into the copy's permissions.
    std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o644)).unwrap();
    let BackupOutcome::Created(version) =
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap()
    else {
        panic!("expected a version");
    };
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&version), 0o600);
    assert_eq!(mode(&sidecar_of(&version)), 0o600);
    assert_eq!(mode(&history), 0o700);
    assert_eq!(mode(&versions_dir_in(&history, &document)), 0o700);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_first_write_has_nothing_to_keep() {
    let dir = scratch("first");
    let history = dir.join("history");
    let document = dir.join("new.op");
    assert_eq!(
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap(),
        BackupOutcome::SkippedMissing
    );
    assert!(!history.exists(), "no history folder for a first write");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unchanged_bytes_are_not_stored_twice() {
    let dir = scratch("dedupe");
    let history = dir.join("history");
    let document = dir.join("a.op");
    std::fs::write(&document, b"same").unwrap();
    assert!(matches!(
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap(),
        BackupOutcome::Created(_)
    ));
    assert_eq!(
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap(),
        BackupOutcome::SkippedIdentical
    );
    assert_eq!(versions_of(&history, &document).len(), 1);
    std::fs::write(&document, b"different").unwrap();
    assert!(matches!(
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap(),
        BackupOutcome::Created(_)
    ));
    assert_eq!(versions_of(&history, &document).len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retention_by_count_drops_the_oldest_and_keeps_the_newest() {
    let dir = scratch("count");
    let history = dir.join("history");
    let document = dir.join("a.op");
    let limits = Limits {
        max_versions: 3,
        ..limits()
    };
    for round in 0..6 {
        std::fs::write(&document, format!("content {round}")).unwrap();
        backup_existing(&history, &document, WrittenBy::CmdS, &limits).unwrap();
    }
    let names = versions_of(&history, &document);
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(content_of_newest(&history, &document), b"content 5");
    // Pruned versions take their sidecar with them.
    let folder = versions_dir_in(&history, &document);
    let sidecars = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .count();
    assert_eq!(sidecars, 3 + 1, "three version sidecars plus meta.json");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retention_by_total_size_prunes_oldest_first_and_never_the_newest_of_a_file() {
    let dir = scratch("total");
    let history = dir.join("history");
    let first = dir.join("first.op");
    let second = dir.join("second.op");
    let roomy = Limits {
        max_total_bytes: u64::MAX,
        ..limits()
    };
    for round in 0..3 {
        std::fs::write(&first, vec![b'a' + round; 100]).unwrap();
        backup_existing(&history, &first, WrittenBy::CmdS, &roomy).unwrap();
    }
    for round in 0..2 {
        std::fs::write(&second, vec![b'x' + round; 100]).unwrap();
        backup_existing(&history, &second, WrittenBy::CmdS, &roomy).unwrap();
    }
    assert_eq!(versions_of(&history, &first).len(), 3);
    assert_eq!(versions_of(&history, &second).len(), 2);

    // Five 100-byte versions; a 250-byte cap leaves two, and a cap of 1 byte
    // still leaves the newest of each file.
    let tight = Limits {
        max_total_bytes: 250,
        ..limits()
    };
    prune(&history, &versions_dir_in(&history, &first), &tight);
    let remaining = versions_of(&history, &first).len() + versions_of(&history, &second).len();
    assert_eq!(remaining, 2, "oldest versions went first");
    let tiny = Limits {
        max_total_bytes: 1,
        ..limits()
    };
    prune(&history, &versions_dir_in(&history, &first), &tiny);
    assert_eq!(versions_of(&history, &first).len(), 1);
    assert_eq!(versions_of(&history, &second).len(), 1);
    assert_eq!(content_of_newest(&history, &first), vec![b'c'; 100]);
    assert_eq!(content_of_newest(&history, &second), vec![b'y'; 100]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_above_the_size_limit_is_skipped() {
    let dir = scratch("large");
    let history = dir.join("history");
    let document = dir.join("a.op");
    std::fs::write(&document, vec![0u8; 64]).unwrap();
    let small = Limits {
        max_file_bytes: 10,
        ..limits()
    };
    assert_eq!(
        backup_existing(&history, &document, WrittenBy::CmdS, &small).unwrap(),
        BackupOutcome::SkippedTooLarge { bytes: 64 }
    );
    assert!(!history.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_symlinked_destination_is_never_read_through() {
    let dir = scratch("symlink-dest");
    let history = dir.join("history");
    let outside = dir.join("outside-secret.txt");
    std::fs::write(&outside, b"not a document").unwrap();
    let document = dir.join("link.op");
    std::os::unix::fs::symlink(&outside, &document).unwrap();
    assert_eq!(
        backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap(),
        BackupOutcome::SkippedNotRegular
    );
    assert!(!history.exists(), "nothing was copied from the link target");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_symlinked_history_folder_is_rejected() {
    let dir = scratch("symlink-history");
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let history = dir.join("history");
    std::os::unix::fs::symlink(&elsewhere, &history).unwrap();
    let document = dir.join("a.op");
    std::fs::write(&document, b"content").unwrap();
    let error = backup_existing(&history, &document, WrittenBy::CmdS, &limits()).unwrap_err();
    assert!(matches!(error, BackupError::UnsafeDirectory(_)), "{error}");
    assert_eq!(
        std::fs::read_dir(&elsewhere).unwrap().count(),
        0,
        "nothing was written through the link"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn version_names_are_utc_and_sort_by_time() {
    assert_eq!(utc_stamp(0), "19700101T000000000Z");
    assert_eq!(utc_stamp(1_000_000_000_000), "20010909T014640000Z");
    // A leap day.
    assert_eq!(utc_stamp(951_782_400_123), "20000229T000000123Z");
    assert!(utc_stamp(1_000_000_000_999) < utc_stamp(1_000_000_001_000));
}

#[test]
fn the_folder_name_is_a_fixed_hash_of_the_path() {
    // Pinned: a different value would orphan every user's existing history.
    let folder = versions_dir_in(
        Path::new("/h"),
        Path::new("/nonexistent-openpencil-dir/a.op"),
    );
    assert_eq!(folder, Path::new("/h/fa6ffd26d35f18506e0b305b0c49a94a"));
}

/// Stage `bytes` beside `destination` and commit them, as a save does.
fn commit(destination: &Path, bytes: &[u8]) -> Result<(), crate::doc_io::DocIoError> {
    let (staged, mut file) = create_sibling_temp(destination).unwrap();
    std::io::Write::write_all(&mut file, bytes).unwrap();
    drop(file);
    crate::doc_io::commit_staged_document(&staged, destination)
}

fn newest_version_of(document: &Path) -> Option<(Vec<u8>, serde_json::Value)> {
    let dir = versions_dir_for_document(document)?;
    let newest = list_versions(&dir).pop()?;
    let path = dir.join(&newest.name);
    let sidecar = serde_json::from_slice(&std::fs::read(sidecar_of(&path)).ok()?).ok()?;
    Some((std::fs::read(path).ok()?, sidecar))
}

#[test]
fn the_save_commit_keeps_the_replaced_bytes_and_a_first_save_keeps_nothing() {
    enable_version_history();
    let dir = scratch("commit");
    let document = dir.join("a.op");
    commit(&document, b"first").unwrap();
    assert!(
        newest_version_of(&document).is_none(),
        "creating a file keeps no version"
    );
    with_origin(WrittenBy::CmdS, || commit(&document, b"second")).unwrap();
    assert_eq!(std::fs::read(&document).unwrap(), b"second");
    let (kept, sidecar) = newest_version_of(&document).expect("the replaced bytes");
    assert_eq!(kept, b"first");
    assert_eq!(sidecar["writtenBy"], "cmd-s");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failing_backup_never_blocks_the_save() {
    enable_version_history();
    let dir = scratch("failing");
    let document = dir.join("a.op");
    std::fs::write(&document, b"old").unwrap();
    // Make the version folder impossible to create: a regular file sits there.
    let folder = versions_dir_for_document(&document).unwrap();
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    std::fs::write(&folder, b"in the way").unwrap();
    commit(&document, b"new").expect("the save goes through");
    assert_eq!(std::fs::read(&document).unwrap(), b"new");
    let _ = std::fs::remove_file(&folder);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_apps_own_files_are_not_backed_up() {
    enable_version_history();
    let settings = crate::settings_io::settings_dir().expect("test settings dir");
    let recovery = settings.join("recovery-test-dir");
    std::fs::create_dir_all(&recovery).unwrap();
    let snapshot = recovery.join("snapshot.op");
    commit(&snapshot, b"one").unwrap();
    commit(&snapshot, b"two").unwrap();
    assert!(
        newest_version_of(&snapshot).is_none(),
        "recovery snapshots are not user documents"
    );
    let _ = std::fs::remove_dir_all(recovery);
}

#[test]
fn an_origin_scope_restores_the_outer_one() {
    assert_eq!(ORIGIN.with(Cell::get), None);
    with_origin(WrittenBy::Autosave, || {
        with_default_origin(WrittenBy::Mcp, || {
            assert_eq!(ORIGIN.with(Cell::get), Some(WrittenBy::Autosave));
        });
        with_origin(WrittenBy::Headless, || {
            assert_eq!(ORIGIN.with(Cell::get), Some(WrittenBy::Headless));
        });
        assert_eq!(ORIGIN.with(Cell::get), Some(WrittenBy::Autosave));
    });
    assert_eq!(ORIGIN.with(Cell::get), None);
}
