use super::*;
use crate::disk_fingerprint::DiskFingerprint;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-save-in-place-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A saved document bound to `file`, plus the baseline of that file.
fn saved_document(file: &Path) -> (EditorState, DiskFingerprint) {
    let state = EditorState::starter();
    crate::doc_io::save_to_path(&state, file).expect("seed file");
    let baseline = DiskFingerprint::capture(file).expect("baseline");
    (state, baseline)
}

#[test]
fn save_in_place_writes_the_document_and_clears_dirty() {
    let dir = scratch("clears");
    let file = dir.join("a.op");
    let (mut state, baseline) = saved_document(&file);
    state.mark_saved_revision();
    state.mark_document_changed();
    assert!(state.is_dirty());

    let saved = save_in_place(&mut state, Some(&file), Some(&baseline)).expect("saves");

    assert!(!state.is_dirty());
    assert!(saved.dirty_before);
    assert_eq!(saved.path, file);
    assert_eq!(saved.bytes, std::fs::metadata(&file).unwrap().len());
    assert_eq!(
        saved.fingerprint,
        DiskFingerprint::capture(&file).unwrap(),
        "the returned fingerprint is the new baseline"
    );
    // A second save from the new baseline is accepted (no false conflict).
    state.mark_document_changed();
    assert!(save_in_place(&mut state, Some(&file), Some(&saved.fingerprint)).is_ok());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_in_place_refuses_untitled_with_actionable_error() {
    let mut state = EditorState::starter();
    state.mark_document_changed();
    let error = save_in_place(&mut state, None, None).expect_err("untitled");
    assert_eq!(error, SaveInPlaceError::Untitled);
    let text = error.to_string();
    assert!(text.contains("no file yet"), "{text}");
    assert!(
        text.contains("create_document { projectRoot, name }"),
        "{text}"
    );
    assert!(text.contains("filePath"), "{text}");
    assert!(state.is_dirty(), "nothing was saved");
}

#[test]
fn save_in_place_never_overwrites_a_file_changed_on_disk() {
    let dir = scratch("changed");
    let file = dir.join("a.op");
    let (mut state, baseline) = saved_document(&file);
    state.mark_document_changed();
    // Someone else rewrites the file after the editor last saw it.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, b"someone else's work").unwrap();

    let error = save_in_place(&mut state, Some(&file), Some(&baseline)).expect_err("conflict");

    assert!(matches!(error, SaveInPlaceError::ChangedOnDisk { .. }));
    assert!(
        error
            .to_string()
            .contains("the file changed on disk since it was opened; open it again or use Save As")
            || error
                .to_string()
                .contains("the file changed on disk since it was opened"),
        "{error}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"someone else's work");
    assert!(state.is_dirty(), "the document keeps its unsaved work");
    // A file deleted behind the editor's back is a change too.
    std::fs::remove_file(&file).unwrap();
    assert!(save_in_place(&mut state, Some(&file), Some(&baseline)).is_err());
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_in_place_refuses_without_a_recorded_baseline() {
    let dir = scratch("nobaseline");
    let file = dir.join("a.op");
    let (mut state, _) = saved_document(&file);
    state.mark_document_changed();
    let error = save_in_place(&mut state, Some(&file), None).expect_err("no baseline");
    assert!(error.is_transient(), "{error}");
    assert!(state.is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_write_keeps_the_old_file_and_the_dirty_mark() {
    let dir = scratch("failure");
    let file = dir.join("a.op");
    let (mut state, baseline) = saved_document(&file);
    let old = std::fs::read(&file).unwrap();
    state.mark_document_changed();
    let error = save_in_place_with(&mut state, Some(&file), Some(&baseline), |_, _| {
        Err(DocIoError::Io("disk full".into()))
    })
    .expect_err("write fails");
    assert!(matches!(error, SaveInPlaceError::Write { .. }), "{error}");
    assert!(state.is_dirty());
    assert_eq!(std::fs::read(&file).unwrap(), old);
    let _ = std::fs::remove_dir_all(dir);
}

/// Invariant: the real writer replaces the file only when the whole new
/// content is on disk. A directory that refuses the sibling temp file leaves
/// the old bytes in place, no temp file behind, and the document dirty. A
/// writer that truncates the target first (`fs::write`) would succeed here
/// and clobber the file in place, which is what this pins against.
#[cfg(unix)]
#[test]
fn save_in_place_is_atomic_and_keeps_old_content_on_write_failure() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("atomic");
    let file = dir.join("a.op");
    let (mut state, baseline) = saved_document(&file);
    let old = std::fs::read(&file).unwrap();
    state.mark_document_changed();
    // Writable file, read-only folder: no sibling temp can be created.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = std::fs::File::create(dir.join("probe"));
    if probe.is_ok() {
        // Running as a user that ignores directory permissions (root): the
        // scenario cannot be produced, so there is nothing to assert.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        return;
    }

    let result = save_in_place(&mut state, Some(&file), Some(&baseline));

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(result, Err(SaveInPlaceError::Write { .. })),
        "{result:?}"
    );
    assert_eq!(std::fs::read(&file).unwrap(), old, "old content intact");
    assert!(state.is_dirty());
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.op".to_string()], "no temp file left");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stale_baseline_refuses_as_changed_on_disk_never_as_a_transient_unknown() {
    let dir = scratch("stale");
    let file = dir.join("a.op");
    let (mut state, _) = saved_document(&file);
    state.mark_document_changed();
    let before = std::fs::read(&file).unwrap();
    let error = save_in_place(&mut state, Some(&file), Some(&DiskFingerprint::Stale))
        .expect_err("a recovered document's first agent save must not overwrite");
    assert!(
        matches!(error, SaveInPlaceError::ChangedOnDisk { .. }),
        "{error}"
    );
    assert!(!error.is_transient(), "a retry cannot clear it");
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(state.is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_in_place_save_keeps_the_replaced_bytes_attributed_to_mcp_or_the_outer_origin() {
    use crate::doc_io::version_backup::{
        enable_version_history, versions_dir_for_document, with_origin, WrittenBy,
    };
    enable_version_history();
    let dir = scratch("origin");
    let file = dir.join("a.op");
    let (mut state, baseline) = saved_document(&file);
    state.doc.name = Some("first".into());
    state.mark_document_changed();
    let saved = save_in_place(&mut state, Some(&file), Some(&baseline)).expect("saves");
    state.doc.name = Some("second".into());
    state.mark_document_changed();
    with_origin(WrittenBy::Autosave, || {
        save_in_place(&mut state, Some(&file), Some(&saved.fingerprint))
    })
    .expect("saves again");
    let folder = versions_dir_for_document(&file).expect("settings dir");
    let mut written_by: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .filter(|entry| entry.file_name() != "meta.json")
        .map(|entry| {
            let sidecar: serde_json::Value =
                serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
            sidecar["writtenBy"].as_str().unwrap().to_string()
        })
        .collect();
    written_by.sort();
    assert_eq!(written_by, vec!["autosave".to_string(), "mcp".to_string()]);
    let _ = std::fs::remove_dir_all(dir);
}
