//! Invariants of opening an empty `.op` file as a blank document.

use super::*;
use op_editor_core::Locale;
use std::path::PathBuf;

fn scratch_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-empty-op-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn open(path: &Path) -> Result<OpenedEditorState, DocIoError> {
    load_initializing_empty_op_file(path, Locale::EnUs)
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .collect();
    names.sort();
    names
}

fn is_the_starter(state: &EditorState) -> bool {
    let children = state.active_children();
    children.len() == 1
        && op_editor_core::PenNodeExt::id_str(&children[0]) == "n10"
        && op_editor_core::PenNodeExt::children(&children[0]).is_some_and(|kids| kids.is_empty())
}

#[test]
fn empty_op_file_opens_as_a_blank_document_and_is_formatted_on_disk() {
    let dir = scratch_dir("zero");
    let path = dir.join("blank.op");
    std::fs::write(&path, b"").unwrap();

    let opened = open(&path).expect("an empty file opens");

    assert!(is_the_starter(&opened.loaded.state), "same blank as New");
    assert_eq!(opened.empty_file, Some(EmptyFileOutcome::Formatted));
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("\"editorMeta\""), "{on_disk}");
    assert!(on_disk.contains("\"n10\""), "{on_disk}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn whitespace_only_and_bom_only_files_count_as_empty() {
    let dir = scratch_dir("blankish");
    let samples: [(&str, &[u8]); 6] = [
        ("spaces.op", b"   "),
        ("newlines.op", b"\r\n\n\t \x0b\x0c\n"),
        ("bom.op", b"\xEF\xBB\xBF"),
        ("bom-space.op", b"\xEF\xBB\xBF \n"),
        ("unicode.op", "\u{00A0}\u{2003}\u{3000}\n".as_bytes()),
        ("UPPER.OP", b"\n"),
    ];
    for (name, content) in samples {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        assert!(is_empty_op_file(&path), "{name} is empty");
        let opened = open(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            opened.empty_file,
            Some(EmptyFileOutcome::Formatted),
            "{name}"
        );
        assert!(is_the_starter(&opened.loaded.state), "{name}");
        assert!(
            !bytes_are_blank(&std::fs::read(&path).unwrap()),
            "{name} was formatted"
        );
    }
    // Two BOMs, a NUL, a lone BOM-lookalike and a directory are not empty.
    for (name, content) in [
        ("two-bom.op", &b"\xEF\xBB\xBF\xEF\xBB\xBF"[..]),
        ("nul.op", b"\0"),
        ("invalid-utf8.op", b"\xFF\xFE "),
        ("zero-width.op", "\u{200B}".as_bytes()),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        assert!(!is_empty_op_file(&path), "{name} is not empty");
    }
    let folder = dir.join("folder.op");
    std::fs::create_dir(&folder).unwrap();
    assert!(!is_empty_op_file(&folder), "a directory is not a file");
    assert!(
        open(&folder).is_err(),
        "a directory keeps the loader's error"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn garbage_partial_and_truncated_files_are_never_touched() {
    let dir = scratch_dir("untouched");
    let mut truncated = Vec::new();
    {
        let source = dir.join("source.op");
        crate::doc_io::save_to_path(&EditorState::starter(), &source).unwrap();
        let full = std::fs::read(&source).unwrap();
        truncated.extend_from_slice(&full[..full.len() / 2]);
    }
    let samples: [(&str, Vec<u8>); 5] = [
        ("garbage.op", b"this is not a document".to_vec()),
        ("partial.op", b"{}".to_vec()),
        ("open-brace.op", b"{".to_vec()),
        ("truncated.op", truncated),
        ("binary.op", vec![0, 159, 146, 150]),
    ];
    for (name, content) in samples {
        let path = dir.join(name);
        std::fs::write(&path, &content).unwrap();
        let today = load_editor_state_with_report(&path, Locale::EnUs).map(|_| ());

        let funnel = open(&path).map(|_| ());

        assert_eq!(funnel, today, "{name}: the same error as today's loader");
        assert!(funnel.is_err(), "{name} still fails");
        assert_eq!(std::fs::read(&path).unwrap(), content, "{name} untouched");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn file_that_gains_content_before_the_write_is_loaded_not_overwritten() {
    let dir = scratch_dir("race");
    let path = dir.join("race.op");
    std::fs::write(&path, b"").unwrap();
    let real = {
        let mut state = EditorState::starter();
        state.mark_document_changed();
        let source = dir.join("real-source.op");
        crate::doc_io::save_to_path(&state, &source).unwrap();
        std::fs::read(&source).unwrap()
    };

    let opened = load_initializing_empty_op_file_with(&path, Locale::EnUs, || {
        std::fs::write(&path, &real).unwrap();
    })
    .expect("the real content loads");

    assert_eq!(opened.empty_file, None, "not reported as initialized");
    assert_eq!(std::fs::read(&path).unwrap(), real, "never overwritten");
    assert!(
        !entries(&dir).iter().any(|name| name.ends_with(".tmp")),
        "{:?}",
        entries(&dir)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn initialization_write_is_atomic_and_leaves_no_temp_file() {
    let dir = scratch_dir("atomic");
    let path = dir.join("atomic.op");
    std::fs::write(&path, b"").unwrap();
    let mut seen_during_write = Vec::new();

    let opened = load_initializing_empty_op_file_with(&path, Locale::EnUs, || {
        // Mid-write the destination is still the untouched empty file.
        seen_during_write = std::fs::read(&path).unwrap();
    })
    .unwrap();

    assert_eq!(opened.empty_file, Some(EmptyFileOutcome::Formatted));
    assert!(seen_during_write.is_empty(), "no half file under the name");
    assert_eq!(entries(&dir), vec!["atomic.op".to_string()]);
    assert!(!bytes_are_blank(&std::fs::read(&path).unwrap()));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn read_only_empty_file_opens_in_memory_dirty_with_a_toast() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch_dir("readonly");
    let file_case = dir.join("locked.op");
    std::fs::write(&file_case, b"").unwrap();
    std::fs::set_permissions(&file_case, std::fs::Permissions::from_mode(0o444)).unwrap();

    let opened = open(&file_case).expect("still opens in memory");

    assert!(is_the_starter(&opened.loaded.state));
    assert!(
        matches!(opened.empty_file, Some(EmptyFileOutcome::NotWritable(_))),
        "{:?}",
        opened.empty_file
    );
    assert_eq!(std::fs::read(&file_case).unwrap(), b"", "left as it was");

    let folder = dir.join("locked-folder");
    std::fs::create_dir(&folder).unwrap();
    let folder_case = folder.join("inside.op");
    std::fs::write(&folder_case, b"").unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    let opened = open(&folder_case);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();

    let opened = opened.expect("a read-only folder still opens in memory");
    assert!(matches!(
        opened.empty_file,
        Some(EmptyFileOutcome::NotWritable(_))
    ));
    assert_eq!(std::fs::read(&folder_case).unwrap(), b"");
    assert_eq!(entries(&folder), vec!["inside.op".to_string()]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn second_open_loads_normally_without_a_notice() {
    let dir = scratch_dir("twice");
    let path = dir.join("twice.op");
    std::fs::write(&path, b"").unwrap();
    assert_eq!(
        open(&path).unwrap().empty_file,
        Some(EmptyFileOutcome::Formatted)
    );
    let after_first = std::fs::read(&path).unwrap();

    let second = open(&path).unwrap();

    assert_eq!(second.empty_file, None, "no notice the second time");
    assert!(is_the_starter(&second.loaded.state));
    assert_eq!(std::fs::read(&path).unwrap(), after_first, "not rewritten");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn empty_non_op_files_keep_todays_behaviour() {
    let dir = scratch_dir("other-ext");
    for name in [
        "a.pen", "a.fig", "a.html", "a.zip", "a.txt", "noext", "a.op.bak",
    ] {
        let path = dir.join(name);
        std::fs::write(&path, b"").unwrap();
        assert!(!is_empty_op_file(&path), "{name}");
        let today = load_editor_state_with_report(&path, Locale::EnUs).map(|_| ());

        let funnel = open(&path).map(|_| ());

        assert_eq!(funnel, today, "{name}");
        assert!(funnel.is_err(), "{name} still fails to load");
        assert_eq!(std::fs::read(&path).unwrap(), b"", "{name} untouched");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_file_keeps_todays_error() {
    let dir = scratch_dir("missing");
    let path = dir.join("missing.op");
    assert_eq!(
        open(&path).map(|_| ()),
        load_editor_state_with_report(&path, Locale::EnUs).map(|_| ())
    );
    assert!(!path.exists(), "nothing is created");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn initialized_file_roundtrips_through_load() {
    let dir = scratch_dir("roundtrip");
    let path = dir.join("roundtrip.op");
    std::fs::write(&path, "  \n").unwrap();
    open(&path).unwrap();

    let reloaded = load_editor_state_with_report(&path, Locale::EnUs).expect("valid .op");

    assert!(is_the_starter(&reloaded.state));
    assert_eq!(
        reloaded.state.doc.pages.as_ref().map(Vec::len),
        EditorState::starter().doc.pages.as_ref().map(Vec::len)
    );
    assert!(
        !reloaded.report.needs_schema_upgrade(),
        "no legacy-upgrade prompt for a freshly formatted file"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_symlink_to_an_empty_file_formats_the_real_file() {
    let dir = scratch_dir("symlink");
    let real = dir.join("real.op");
    let link = dir.join("link.op");
    std::fs::write(&real, b"").unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let opened = open(&link).unwrap();

    assert_eq!(opened.empty_file, Some(EmptyFileOutcome::Formatted));
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link is kept"
    );
    assert!(!bytes_are_blank(&std::fs::read(&real).unwrap()));
    let _ = std::fs::remove_dir_all(dir);
}
