//! Creating a design: names never collide, failures leave nothing behind.

use super::*;
use op_host_services::doc_io::DocIoError;
use std::cell::Cell;

fn temp_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-newdesign-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.canonicalize().unwrap()
}

fn real_write(target: &Path) -> Result<(), SaveNewError> {
    op_host_services::doc_io::save_new_to_path(&op_editor_core::EditorState::starter(), target)
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|read| {
            read.filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn next_untitled_design_never_reuses_a_name() {
    let root = temp_root("names");
    assert!(next_untitled_design(&root).ends_with("designs/untitled.op"));
    create_design_file(&root, &real_write).unwrap();
    assert!(next_untitled_design(&root).ends_with("designs/untitled-2.op"));
    create_design_file(&root, &real_write).unwrap();
    assert!(next_untitled_design(&root).ends_with("designs/untitled-3.op"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_design_retries_on_collision() {
    let root = temp_root("collision");
    let designs = root.join("designs");
    let raced = Cell::new(false);
    // An agent creates `untitled.op` between the name being chosen and the
    // write: the write must refuse to overwrite it and move on.
    let write = |target: &Path| {
        if !raced.replace(true) {
            std::fs::write(target, "agent's file").unwrap();
        }
        real_write(target)
    };
    let created = create_design_file(&root, &write).unwrap();
    assert!(created.ends_with("designs/untitled-2.op"));
    assert_eq!(
        std::fs::read_to_string(designs.join("untitled.op")).unwrap(),
        "agent's file",
        "the file that won the race is untouched"
    );
    assert!(created.is_file());
    assert_eq!(entries(&designs), ["untitled-2.op", "untitled.op"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn new_design_gives_up_after_the_attempt_limit_without_creating_anything() {
    let root = temp_root("exhausted");
    let attempts = Cell::new(0usize);
    let write = |_: &Path| {
        attempts.set(attempts.get() + 1);
        Err(SaveNewError::AlreadyExists)
    };
    assert_eq!(
        create_design_file(&root, &write),
        Err(NewDesignError::NamesTaken)
    );
    assert_eq!(attempts.get(), MAX_NAME_ATTEMPTS);
    assert!(
        !root.join("designs").exists(),
        "a folder this call created is removed again"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_write_removes_the_folder_it_created_but_not_an_existing_one() {
    let failing = |_: &Path| Err(SaveNewError::Io(DocIoError::Io("disk full".to_string())));

    let fresh = temp_root("write-fails-fresh");
    assert!(matches!(
        create_design_file(&fresh, &failing),
        Err(NewDesignError::Write(_))
    ));
    assert!(!fresh.join("designs").exists());

    let existing = temp_root("write-fails-existing");
    std::fs::create_dir_all(existing.join("designs")).unwrap();
    std::fs::write(existing.join("designs/keep.op"), "{}").unwrap();
    assert!(create_design_file(&existing, &failing).is_err());
    assert_eq!(entries(&existing.join("designs")), ["keep.op"]);
    let _ = std::fs::remove_dir_all(fresh);
    let _ = std::fs::remove_dir_all(existing);
}

#[test]
fn a_file_in_the_way_of_the_designs_folder_is_a_create_folder_error() {
    let root = temp_root("file-in-the-way");
    std::fs::write(root.join("designs"), "not a folder").unwrap();
    let outcome = create_design_file(&root, &real_write);
    assert!(matches!(outcome, Err(NewDesignError::CreateFolder(_))));
    assert_eq!(entries(&root), ["designs"]);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn a_designs_link_leading_out_of_the_project_is_refused() {
    let root = temp_root("link-root");
    let elsewhere = temp_root("link-elsewhere");
    std::os::unix::fs::symlink(&elsewhere, root.join("designs")).unwrap();
    assert_eq!(
        create_design_file(&root, &real_write),
        Err(NewDesignError::FolderOutsideProject)
    );
    assert!(
        entries(&elsewhere).is_empty(),
        "nothing was written outside"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(elsewhere);
}
