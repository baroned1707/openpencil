//! What converting a listed `.fig` decides from the disk: refusals, an `.op`
//! that is already there, and a free output name.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A canonical project root with nothing in it.
pub(super) fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-fig-plan-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn fig(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"fig").unwrap();
    path
}

#[test]
fn fig_with_sibling_op_is_converted() {
    let root = project("sibling");
    let listed = fig(&root, "screens/home.fig");
    std::fs::write(root.join("screens/home.op"), b"{}").unwrap();
    assert_eq!(
        plan_conversion(&root, &listed),
        Ok(Plan::AlreadyConverted(root.join("screens/home.op"))),
        "nothing to convert: the click opens the .op"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_fig_without_a_sibling_converts_next_to_itself() {
    let root = project("plain");
    let listed = fig(&root, "screens/home.fig");
    let Ok(Plan::Convert { source, output }) = plan_conversion(&root, &listed) else {
        panic!("expected a conversion");
    };
    assert_eq!(source, listed);
    assert_eq!(output, root.join("screens/home.op"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn collision_gets_a_unique_name() {
    let root = project("collision");
    let listed = fig(&root, "a.fig");
    // `a.op` is a FOLDER: occupied, but not a design that could be opened.
    std::fs::create_dir(root.join("a.op")).unwrap();
    let Ok(Plan::Convert { output, .. }) = plan_conversion(&root, &listed) else {
        panic!("expected a conversion");
    };
    assert_eq!(output, root.join("a-converted.op"));
    std::fs::write(root.join("a-converted.op"), b"x").unwrap();
    std::fs::write(root.join("a-converted-2.op"), b"x").unwrap();
    let Ok(Plan::Convert { output, .. }) = plan_conversion(&root, &listed) else {
        panic!("expected a conversion");
    };
    assert_eq!(output, root.join("a-converted-3.op"));
    // The proposer itself: a name is free only when `taken` says so.
    let taken = |path: &Path| {
        path.file_name()
            .is_some_and(|name| name != "a-converted-9.op")
    };
    assert_eq!(
        unique_output_path(&root, std::ffi::OsStr::new("a"), &taken),
        Some(root.join("a-converted-9.op"))
    );
    assert_eq!(
        unique_output_path(&root, std::ffi::OsStr::new("a"), &|_| true),
        None,
        "gives up instead of looping forever"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_escape_fig_is_refused() {
    let root = project("escape");
    let outside = project("escape-outside");
    let listed = fig(&root, "inside.fig");
    // Listed as a regular file; swapped for a link out of the project before
    // the conversion starts (the scan can no longer vouch for it).
    std::fs::remove_file(&listed).unwrap();
    let secret = fig(&outside, "secret.fig");
    std::os::unix::fs::symlink(&secret, &listed).unwrap();
    assert_eq!(
        plan_conversion(&root, &listed),
        Err(Refusal::OutsideProject)
    );
    // The same for the folder: a linked folder that leaves the project.
    let linked_folder = root.join("linked");
    std::os::unix::fs::symlink(&outside, &linked_folder).unwrap();
    assert_eq!(
        plan_conversion(&root, &linked_folder.join("secret.fig")),
        Err(Refusal::OutsideProject)
    );
    assert!(
        !outside.join("secret.op").exists(),
        "nothing was written outside the project"
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn only_a_regular_fig_file_inside_the_size_limit_is_planned() {
    let root = project("refusals");
    std::fs::create_dir(root.join("d.fig")).unwrap();
    assert_eq!(
        plan_conversion(&root, &root.join("d.fig")),
        Err(Refusal::NotARegularFile)
    );
    assert_eq!(
        plan_conversion(&root, &root.join("notes.txt")),
        Err(Refusal::NotAFigFile)
    );
    assert!(matches!(
        plan_conversion(&root, &root.join("gone.fig")),
        Err(Refusal::Unreadable(_))
    ));
    let huge = root.join("huge.fig");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_INPUT_BYTES + 1)
        .unwrap();
    assert_eq!(plan_conversion(&root, &huge), Err(Refusal::TooLarge));
    assert_eq!(Refusal::TooLarge.to_string(), "too large");
    let _ = std::fs::remove_dir_all(root);
}
