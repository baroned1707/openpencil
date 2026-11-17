//! The recursive project walk: what it lists, what it never enters, and how it
//! reports a cut. Every case builds its own tree under the OS temp dir.

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-scan-tree-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }

    fn file(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        path
    }

    fn relative(&self, listing: &DesignListing) -> Vec<String> {
        listing
            .files
            .iter()
            .map(|path| {
                path.strip_prefix(&self.0)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scan(tree: &Tree) -> DesignListing {
    scan_project_tree(&tree.0, &TreeLimits::default())
}

#[test]
fn every_op_file_anywhere_is_listed_sorted_by_relative_path() {
    let tree = Tree::new();
    for relative in [
        "z-last.op",
        "a-first.op",
        "designs/home.op",
        "screens/auth/login.op",
        "screens/auth/deep/er/signup.op",
        "screens/cart.op",
        "notes.txt",
        "screens/readme.md",
    ] {
        tree.file(relative);
    }
    let listing = scan(&tree);
    assert_eq!(listing.status, ScanStatus::Ok);
    assert_eq!(
        tree.relative(&listing),
        [
            "a-first.op",
            "designs/home.op",
            "screens/auth/deep/er/signup.op",
            "screens/auth/login.op",
            "screens/cart.op",
            "z-last.op",
        ]
    );
}

#[test]
fn scan_skips_heavy_and_hidden_directories() {
    let tree = Tree::new();
    tree.file("keep.op");
    for skipped in SKIPPED_DIRECTORIES {
        tree.file(&format!("{skipped}/inside.op"));
        tree.file(&format!("nested/{skipped}/inside.op"));
    }
    tree.file(".hidden-dir/inside.op");
    tree.file("nested/.another/inside.op");
    tree.file(".hidden-file.op");
    tree.file("nested/visible.op");
    let listing = scan(&tree);
    assert_eq!(tree.relative(&listing), ["keep.op", "nested/visible.op"]);
    assert_eq!(listing.status, ScanStatus::Ok);
}

#[test]
fn scan_depth_and_file_caps_report_truncation() {
    // File cap: the first files by relative path are listed, the rest counted.
    let tree = Tree::new();
    for n in 0..10 {
        tree.file(&format!("f{n}.op"));
    }
    let limits = TreeLimits {
        max_files: 4,
        ..TreeLimits::default()
    };
    let listing = scan_project_tree(&tree.0, &limits);
    assert_eq!(
        tree.relative(&listing),
        ["f0.op", "f1.op", "f2.op", "f3.op"],
        "the cut is by relative path, not by directory order"
    );
    assert_eq!(listing.status, ScanStatus::Truncated(6));

    // Depth cap: directories past it are not entered, but the `.op` files
    // directly inside them are counted as left out.
    let tree = Tree::new();
    tree.file("top.op");
    tree.file("one/two.op");
    tree.file("one/two/three.op");
    tree.file("one/two/three/four.op");
    let limits = TreeLimits {
        max_depth: 2,
        ..TreeLimits::default()
    };
    let listing = scan_project_tree(&tree.0, &limits);
    assert_eq!(
        tree.relative(&listing),
        // Path order is component-wise: `one/two/` sorts before `one/two.op`.
        ["one/two/three.op", "one/two.op", "top.op"]
    );
    assert_eq!(
        listing.status,
        ScanStatus::Truncated(1),
        "`one/two/three/four.op` sits beyond the cap"
    );
}

#[test]
fn a_walk_out_of_time_returns_what_it_found_marked_slow() {
    let tree = Tree::new();
    tree.file("a.op");
    let limits = TreeLimits {
        budget: Duration::ZERO,
        ..TreeLimits::default()
    };
    let listing = scan_project_tree(&tree.0, &limits);
    assert_eq!(listing.status, ScanStatus::Slow);

    let limits = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    for n in 0..5 {
        tree.file(&format!("more/f{n}.op"));
    }
    assert_eq!(scan_project_tree(&tree.0, &limits).status, ScanStatus::Slow);
}

#[test]
fn a_missing_root_is_unreadable_not_empty() {
    let tree = Tree::new();
    let listing = scan_project_tree(&tree.0.join("gone"), &TreeLimits::default());
    assert!(matches!(listing.status, ScanStatus::Unreadable(_)));
    assert!(listing.files.is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_pointing_outside_the_root_is_ignored_and_loops_do_not_hang() {
    use std::os::unix::fs::symlink;
    let tree = Tree::new();
    let outside = Tree::new();
    let secret = outside.file("secret.op");
    outside.file("folder/other.op");
    tree.file("inside/real.op");

    // A file link to a file outside, a directory link to a folder outside, and
    // a file link to a file inside the root.
    symlink(&secret, tree.0.join("leak.op")).unwrap();
    symlink(outside.0.join("folder"), tree.0.join("leaked-dir")).unwrap();
    symlink(tree.0.join("inside/real.op"), tree.0.join("alias.op")).unwrap();
    // Loops: a directory linking to its own ancestor, and a link to itself.
    symlink(&tree.0, tree.0.join("inside/loop")).unwrap();
    symlink(tree.0.join("self.op"), tree.0.join("self.op")).unwrap();
    // A dangling link.
    symlink(tree.0.join("nowhere.op"), tree.0.join("dangling.op")).unwrap();

    let started = Instant::now();
    let listing = scan(&tree);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the loop must not hang"
    );
    assert_eq!(
        tree.relative(&listing),
        ["alias.op", "inside/real.op"],
        "only files whose target is a file inside the root"
    );
    assert_eq!(listing.status, ScanStatus::Ok);
}
