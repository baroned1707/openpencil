use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openpencil-rename-fs-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
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

#[test]
fn rename_keeps_extension_and_moves_the_file() {
    let root = temp_root("move");
    let source = root.join("designs/card.op");
    write(&source, "card body");
    let plan = plan_rename(&[root.clone()], &source, "  hero  ")
        .unwrap()
        .expect("a different name");
    assert_eq!(plan.target, root.join("designs/hero.op"));
    assert!(!plan.case_only);
    execute_rename(&plan).unwrap();
    assert_eq!(names(&root.join("designs")), ["hero.op"]);
    assert_eq!(
        std::fs::read_to_string(root.join("designs/hero.op")).unwrap(),
        "card body"
    );
    // A typed extension is not doubled, and the file keeps its own extension.
    let again = plan_rename(&[root.clone()], &root.join("designs/hero.op"), "final.op")
        .unwrap()
        .unwrap();
    assert_eq!(again.target, root.join("designs/final.op"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_same_name_is_nothing_to_do() {
    let root = temp_root("same");
    let source = root.join("a.op");
    write(&source, "x");
    assert_eq!(plan_rename(&[root.clone()], &source, "a").unwrap(), None);
    assert_eq!(plan_rename(&[root.clone()], &source, "a.op").unwrap(), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn invalid_names_are_rejected_without_touching_disk() {
    let root = temp_root("invalid");
    let source = root.join("card.op");
    write(&source, "body");
    write(&root.join("other.op"), "other");
    let before = names(&root);
    let long = "a".repeat(200);
    let table: [(&str, RenameNameError); 7] = [
        ("", RenameNameError::Empty),
        ("..", RenameNameError::DotName),
        ("a/b", RenameNameError::Separator),
        (".hidden", RenameNameError::LeadingDot),
        ("a\0b", RenameNameError::Control),
        (long.as_str(), RenameNameError::TooLong),
        ("trailing.", RenameNameError::TrailingDotOrSpace),
    ];
    for (draft, expected) in table {
        assert_eq!(
            plan_rename(&[root.clone()], &source, draft),
            Err(RenameRefusal::InvalidName(expected)),
            "{draft:?}"
        );
    }
    assert_eq!(names(&root), before, "the disk was not touched");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "body");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rename_never_overwrites_an_existing_file() {
    let root = temp_root("overwrite");
    let source = root.join("card.op");
    let taken = root.join("hero.op");
    write(&source, "card body");
    write(&taken, "hero body");
    // The check says no, ignoring case.
    for draft in ["hero", "HERO", "Hero.op"] {
        assert_eq!(
            plan_rename(&[root.clone()], &source, draft),
            Err(RenameRefusal::NameTaken),
            "{draft}"
        );
    }
    // The rename primitive says no by itself, whatever was checked before.
    let error = rename_no_replace(&source, &taken).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    // And the planned rename is refused when the name appears after the plan.
    let plan = plan_rename(&[root.clone()], &source, "late")
        .unwrap()
        .unwrap();
    write(&plan.target, "arrived meanwhile");
    assert_eq!(execute_rename(&plan), Err(RenameRefusal::NameTaken));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "card body");
    assert_eq!(std::fs::read_to_string(&taken).unwrap(), "hero body");
    assert_eq!(
        std::fs::read_to_string(&plan.target).unwrap(),
        "arrived meanwhile"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn case_only_rename_works_on_a_case_insensitive_volume() {
    let root = temp_root("case");
    // Ask the volume, not the platform: does `PROBE` also answer to `probe`?
    write(&root.join("PROBE.tmp"), "p");
    let insensitive = root.join("probe.tmp").exists();
    std::fs::remove_file(root.join("PROBE.tmp")).unwrap();

    let source = root.join("Card.op");
    write(&source, "card body");
    let plan = plan_rename(&[root.clone()], &source, "card")
        .unwrap()
        .expect("a case change is a rename");
    assert!(plan.case_only);
    execute_rename(&plan).unwrap();
    assert_eq!(
        names(&root),
        ["card.op"],
        "the listing shows the new case (case-insensitive volume: {insensitive})"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("card.op")).unwrap(),
        "card body"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_name_that_differs_only_by_case_from_another_file_is_a_collision() {
    let root = temp_root("case-collide");
    write(&root.join("Hero.op"), "h");
    let source = root.join("card.op");
    write(&source, "c");
    assert_eq!(
        plan_rename(&[root.clone()], &source, "hero"),
        Err(RenameRefusal::NameTaken)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn forged_request_outside_the_project_roots_is_refused() {
    let root = temp_root("root");
    let elsewhere = temp_root("elsewhere");
    let outside = elsewhere.join("secret.op");
    write(&outside, "secret");
    assert_eq!(
        plan_rename(&[root.clone()], &outside, "taken"),
        Err(RenameRefusal::OutsideProjects)
    );
    // A `..` hop out of the root is outside too.
    write(&root.join("inside.op"), "i");
    let hop = root
        .join("../")
        .join(elsewhere.file_name().unwrap())
        .join("secret.op");
    assert_eq!(
        plan_rename(&[root.clone()], &hop, "taken"),
        Err(RenameRefusal::OutsideProjects)
    );
    // No roots shown at all: nothing is renamable.
    assert_eq!(
        plan_rename(&[], &root.join("inside.op"), "x"),
        Err(RenameRefusal::OutsideProjects)
    );
    assert_eq!(names(&elsewhere), ["secret.op"]);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(elsewhere);
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_refused() {
    let root = temp_root("link-root");
    let elsewhere = temp_root("link-out");
    write(&elsewhere.join("real.op"), "real");
    // A file link inside the root that leads out.
    std::os::unix::fs::symlink(elsewhere.join("real.op"), root.join("link.op")).unwrap();
    assert_eq!(
        plan_rename(&[root.clone()], &root.join("link.op"), "renamed"),
        Err(RenameRefusal::NotADesignFile)
    );
    // A folder link inside the root that leads out.
    std::os::unix::fs::symlink(&elsewhere, root.join("sub")).unwrap();
    assert_eq!(
        plan_rename(&[root.clone()], &root.join("sub/real.op"), "renamed"),
        Err(RenameRefusal::OutsideProjects)
    );
    assert_eq!(names(&elsewhere), ["real.op"]);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(elsewhere);
}

#[test]
fn only_regular_op_files_are_renamable() {
    let root = temp_root("kinds");
    write(&root.join("brand.fig"), "fig");
    write(&root.join("notes.txt"), "txt");
    std::fs::create_dir_all(root.join("folder.op")).unwrap();
    for name in ["brand.fig", "notes.txt", "folder.op"] {
        assert_eq!(
            plan_rename(&[root.clone()], &root.join(name), "x"),
            Err(RenameRefusal::NotADesignFile),
            "{name}"
        );
    }
    assert_eq!(
        plan_rename(&[root.clone()], &root.join("gone.op"), "x"),
        Err(RenameRefusal::SourceGone)
    );
    let _ = std::fs::remove_dir_all(root);
}
