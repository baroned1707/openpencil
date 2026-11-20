use super::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-disk-baselines-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn a_file_is_recorded_once_and_a_closed_file_is_forgotten() {
    let dir = scratch("sweep");
    let file = dir.join("a.op");
    std::fs::write(&file, b"one").unwrap();
    let mut baselines = DiskBaselines::default();

    baselines.sweep(std::slice::from_ref(&file));
    let first = baselines.get(&file).expect("recorded on first sight");

    // The file changes behind the editor: a later sweep must NOT absorb it.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, b"two!").unwrap();
    baselines.sweep(std::slice::from_ref(&file));
    assert_eq!(baselines.get(&file), Some(first), "never re-captured");

    // Closed: the entry goes, so a reopened file starts fresh.
    baselines.sweep(&[]);
    assert_eq!(baselines.get(&file), None);
    baselines.sweep(std::slice::from_ref(&file));
    assert_ne!(baselines.get(&file), Some(first));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_editors_own_write_becomes_the_baseline() {
    let dir = scratch("record");
    let file = dir.join("a.op");
    std::fs::write(&file, b"one").unwrap();
    let mut baselines = DiskBaselines::default();
    baselines.sweep(std::slice::from_ref(&file));
    std::fs::write(&file, b"saved by the editor").unwrap();
    baselines.record_from_disk(&file);
    assert_eq!(baselines.get(&file), DiskFingerprint::capture(&file).ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_renamed_file_keeps_its_record_and_a_stale_one_stays_stale() {
    let dir = scratch("rename");
    let (old, new) = (dir.join("a.op"), dir.join("b.op"));
    std::fs::write(&old, b"one").unwrap();
    let mut baselines = DiskBaselines::default();
    baselines.record_from_disk(&old);
    let record = baselines.get(&old);
    std::fs::rename(&old, &new).unwrap();
    baselines.rename(&old, &new);
    assert_eq!(baselines.get(&old), None);
    assert_eq!(baselines.get(&new), record);
    assert_eq!(baselines.get(&new), DiskFingerprint::capture(&new).ok());
    baselines.mark_stale(&new);
    let renamed_again = dir.join("c.op");
    baselines.rename(&new, &renamed_again);
    assert_eq!(baselines.get(&renamed_again), Some(DiskFingerprint::Stale));
    let _ = std::fs::remove_dir_all(dir);
}
