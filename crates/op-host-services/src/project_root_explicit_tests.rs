//! The explicit-root rule: deepest root wins, the memo does not go stale, and
//! unsafe folders are refused. Every case builds its own tree under the OS temp
//! dir (canonicalized) and injects the home directory and system folders.

use super::*;
use crate::project_root::canonical_home;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-explicit-roots-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let dir = self.0.join(relative);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_document_belongs_to_the_deepest_explicit_root_containing_it() {
    let tree = Tree::new();
    let outer = tree.dir("repo");
    let inner = tree.dir("repo/apps/web");
    let sibling = tree.dir("other");
    let roots = ExplicitRoots::new();
    assert!(roots.insert(outer.clone()));
    assert!(roots.insert(inner.clone()));
    assert!(!roots.insert(inner.clone()), "a repeat is not a change");

    assert_eq!(roots.root_for(&inner.join("a.op")), Some(inner.clone()));
    assert_eq!(
        roots.root_for(&inner.join("deep/er/b.op")),
        None,
        "no such folder"
    );
    let deeper = tree.dir("repo/apps/web/screens");
    assert_eq!(roots.root_for(&deeper.join("b.op")), Some(inner));
    assert_eq!(roots.root_for(&outer.join("c.op")), Some(outer));
    assert_eq!(roots.root_for(&sibling.join("d.op")), None);
}

#[test]
fn a_root_applies_without_any_marker_file_and_beats_a_nearer_marker() {
    let tree = Tree::new();
    let root = tree.dir("plain");
    let nested = tree.dir("plain/pkg");
    std::fs::write(nested.join("package.json"), "{}").unwrap();
    let roots = ExplicitRoots::new();
    roots.insert(root.clone());
    // The marker would have made `plain/pkg` the project; the explicit root
    // is the user's word and wins.
    assert_eq!(roots.root_for(&nested.join("a.op")), Some(root));
}

#[test]
fn changes_bump_the_generation_and_removal_stops_the_override() {
    let tree = Tree::new();
    let root = tree.dir("p");
    let roots = ExplicitRoots::new();
    let start = roots.generation();
    roots.insert(root.clone());
    assert_ne!(roots.generation(), start);
    assert_eq!(roots.root_for(&root.join("a.op")), Some(root.clone()));
    let after_insert = roots.generation();
    assert!(roots.remove(&root));
    assert!(!roots.remove(&root));
    assert_ne!(roots.generation(), after_insert);
    assert_eq!(roots.root_for(&root.join("a.op")), None);
    assert!(roots.is_empty());
}

#[test]
fn the_canonical_parent_is_read_once_per_path() {
    let tree = Tree::new();
    let root = tree.dir("p");
    let roots = ExplicitRoots::new();
    roots.insert(root.clone());
    let path = root.join("a.op");
    assert_eq!(roots.root_for(&path), Some(root.clone()));
    // Remove the folder: the memoized answer is served without the disk.
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(roots.root_for(&path), Some(root));
}

fn system(tree: &Tree) -> SystemDirs {
    SystemDirs {
        exact: vec![tree.dir("sys-exact")],
        trees: vec![tree.dir("System")],
    }
}

#[test]
fn unsafe_roots_are_the_filesystem_root_home_its_ancestors_and_system_folders() {
    let tree = Tree::new();
    let home = tree.dir("Users/me");
    let system = system(&tree);
    assert_eq!(
        check_project_root(Path::new("/"), Some(&home), &system),
        Err(UnsafeRoot::FilesystemRoot)
    );
    assert_eq!(
        check_project_root(&home, Some(&home), &system),
        Err(UnsafeRoot::HomeOrAncestor)
    );
    assert_eq!(
        check_project_root(&tree.0.join("Users"), Some(&home), &system),
        Err(UnsafeRoot::HomeOrAncestor),
        "a folder holding home would list every user's files"
    );
    assert_eq!(
        check_project_root(&tree.0.join("sys-exact"), Some(&home), &system),
        Err(UnsafeRoot::SystemDirectory)
    );
    assert_eq!(
        check_project_root(&tree.dir("System/Library/x"), Some(&home), &system),
        Err(UnsafeRoot::SystemDirectory),
        "a tree is refused with everything inside"
    );
}

#[test]
fn ordinary_folders_are_fine_including_children_of_an_exact_system_folder() {
    let tree = Tree::new();
    let home = tree.dir("Users/me");
    let system = system(&tree);
    let inside_exact = tree.dir("sys-exact/folders/abc");
    assert_eq!(
        check_project_root(&inside_exact, Some(&home), &system),
        Ok(())
    );
    assert_eq!(
        check_project_root(&tree.dir("Users/me/work/shop"), Some(&home), &system),
        Ok(())
    );
    assert_eq!(
        check_project_root(&tree.dir("elsewhere"), None, &system),
        Ok(())
    );
}

#[test]
fn this_machines_system_folders_cover_the_usual_suspects_but_not_the_temp_dir() {
    let host = SystemDirs::host();
    let home = canonical_home();
    for name in ["/", "/usr", "/etc"] {
        if let Ok(path) = std::fs::canonicalize(name) {
            assert!(
                check_project_root(&path, home.as_deref(), &host).is_err(),
                "{name} must be refused"
            );
        }
    }
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let scratch = temp.join(format!("openpencil-system-probe-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = std::fs::canonicalize(scratch).unwrap();
    let verdict = check_project_root(&scratch, home.as_deref(), &host);
    let _ = std::fs::remove_dir_all(&scratch);
    assert_eq!(verdict, Ok(()), "a temp folder is an ordinary project root");
}
