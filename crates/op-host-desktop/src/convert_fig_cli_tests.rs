//! The child-process side of `.fig` conversion: what `convert_fig_to` writes,
//! what it refuses and what it never touches. The conversion itself is injected
//! (no real `.fig` fixture lives in the repo); the writing is the real one.

use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-convert-fig-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn starter(_: &Path) -> Result<EditorState, String> {
    Ok(EditorState::starter())
}

#[test]
fn convert_writes_atomically_and_leaves_the_fig_untouched() {
    let dir = folder("atomic");
    let fig = dir.join("a.fig");
    let output = dir.join("a.op");
    std::fs::write(&fig, b"not really figma").unwrap();
    let during = std::cell::Cell::new(None);
    let result = convert_fig_to(&fig, &output, &|input| {
        // Nothing is at the final name while the conversion runs.
        during.set(Some(output.exists()));
        starter(input)
    });
    assert_eq!(result, Ok(()));
    assert_eq!(during.get(), Some(false));
    assert!(
        op_host_services::doc_io::load_editor_state(&output, op_editor_core::Locale::EnUs).is_ok(),
        "the output is a complete, loadable document"
    );
    assert_eq!(std::fs::read(&fig).unwrap(), b"not really figma");
    assert_eq!(
        names(&dir),
        ["a.fig", "a.op"],
        "no temp file is left behind"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_child_never_overwrites_an_existing_output() {
    let dir = folder("exists");
    let fig = dir.join("a.fig");
    let output = dir.join("a.op");
    std::fs::write(&fig, b"fig").unwrap();
    std::fs::write(&output, b"precious").unwrap();
    let converted = std::cell::Cell::new(false);
    let result = convert_fig_to(&fig, &output, &|input| {
        converted.set(true);
        starter(input)
    });
    assert_eq!(result, Err(ConvertFigError::OutputExists));
    assert!(!converted.get(), "refused before any conversion work");
    assert_eq!(std::fs::read(&output).unwrap(), b"precious");

    // The name taken while the conversion ran: still not replaced, no temp left.
    let racing = dir.join("b.op");
    let result = convert_fig_to(&fig, &racing, &|input| {
        std::fs::write(&racing, b"won the race").unwrap();
        starter(input)
    });
    assert_eq!(result, Err(ConvertFigError::OutputExists));
    assert_eq!(std::fs::read(&racing).unwrap(), b"won the race");
    assert_eq!(names(&dir), ["a.fig", "a.op", "b.op"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_conversion_leaves_no_output() {
    let dir = folder("failed");
    let fig = dir.join("a.fig");
    std::fs::write(&fig, b"junk").unwrap();
    let output = dir.join("a.op");
    let result = convert_fig_to(&fig, &output, &|_| Err("bad kiwi header".to_string()));
    assert_eq!(
        result,
        Err(ConvertFigError::Convert("bad kiwi header".into()))
    );
    assert_eq!(names(&dir), ["a.fig"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn only_a_regular_file_within_the_size_limit_is_converted() {
    let dir = folder("input");
    let output = dir.join("out.op");
    let never = |_: &Path| -> Result<EditorState, String> { panic!("must not convert") };
    // A folder is not a file.
    let folder_named_fig = dir.join("d.fig");
    std::fs::create_dir(&folder_named_fig).unwrap();
    assert_eq!(
        convert_fig_to(&folder_named_fig, &output, &never),
        Err(ConvertFigError::NotARegularFile)
    );
    // Missing.
    assert_eq!(
        convert_fig_to(&dir.join("gone.fig"), &output, &never),
        Err(ConvertFigError::NotARegularFile)
    );
    // One byte over the limit (a sparse file: no disk is used).
    let huge = dir.join("huge.fig");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_INPUT_BYTES + 1)
        .unwrap();
    assert_eq!(
        convert_fig_to(&huge, &output, &never),
        Err(ConvertFigError::TooLarge)
    );
    assert!(!output.exists());
    let _ = std::fs::remove_dir_all(dir);
}
