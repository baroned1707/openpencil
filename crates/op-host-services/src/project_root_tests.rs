//! Tests for `project_root.rs`. Every case builds its own directory tree under
//! the OS temp dir (canonicalized, since macOS temp lives behind a symlink) and
//! injects a fake home so the machine's real `~/.claude` cannot interfere.

use super::*;
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

static TREE_SEQ: AtomicU32 = AtomicU32::new(0);

/// A throwaway directory removed on drop.
struct Tree(PathBuf);

impl Tree {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "openpencil-project-root-{}-{}",
            std::process::id(),
            TREE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn dir(&self, relative: &str) -> PathBuf {
        let dir = self.0.join(relative);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn root_is_nearest_marker_ancestor() {
    let tree = Tree::new();
    let home = tree.dir("home");
    let outer = tree.dir("home/work/outer");
    let inner = tree.dir("home/work/outer/packages/inner/designs");
    fs::create_dir_all(outer.join(".git")).unwrap();
    fs::write(outer.join("packages/inner/package.json"), "{}").unwrap();

    let found = project_root_with_home(&inner.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, outer.join("packages/inner"));
    assert_eq!(found.name, "inner");
    assert_eq!(found.source, ProjectRootSource::Marker("package.json"));

    // From a sibling directory the outer `.git` is the nearest marker.
    let sibling = tree.dir("home/work/outer/docs");
    let found = project_root_with_home(&sibling.join("b.op"), Some(&home)).unwrap();
    assert_eq!(found.root, outer);
    assert_eq!(found.source, ProjectRootSource::Marker(".git"));
}

#[test]
fn every_marker_is_recognised() {
    for marker in PROJECT_MARKERS {
        let tree = Tree::new();
        let home = tree.dir("home");
        let project = tree.dir("home/proj");
        let docs = tree.dir("home/proj/docs");
        if marker == ".git" || marker == ".claude" {
            fs::create_dir_all(project.join(marker)).unwrap();
        } else {
            fs::write(project.join(marker), "").unwrap();
        }
        let found = project_root_with_home(&docs.join("x.op"), Some(&home)).unwrap();
        assert_eq!(found.root, project, "marker {marker}");
        assert_eq!(found.source, ProjectRootSource::Marker(marker));
    }
}

#[test]
fn never_returns_home_or_filesystem_root() {
    let tree = Tree::new();
    // Home carries a global `.claude` and `.git`, as real homes often do.
    let home = tree.dir("home");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".git")).unwrap();
    let docs = tree.dir("home/Documents/notes");

    let found = project_root_with_home(&docs.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, docs, "home markers must be ignored");
    assert_eq!(found.source, ProjectRootSource::FileDir);

    // A document directly inside home has no project directory at all.
    assert_eq!(
        project_root_with_home(&home.join("a.op"), Some(&home)),
        None
    );

    // Nor does one directly under the filesystem root.
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .ancestors()
        .last()
        .unwrap()
        .to_path_buf();
    assert_eq!(
        project_root_with_home(&root.join("a.op"), Some(&home)),
        None
    );
}

#[test]
fn falls_back_to_file_dir_without_markers() {
    let tree = Tree::new();
    let home = tree.dir("home");
    let docs = tree.dir("home/loose/drafts");

    let found = project_root_with_home(&docs.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, docs);
    assert_eq!(found.name, "drafts");
    assert_eq!(found.source, ProjectRootSource::FileDir);
}

#[test]
fn designs_folder_without_markers_resolves_to_its_parent() {
    // `create_document` writes `<root>/designs/<name>.op`; the agent that made
    // it must keep `<root>` as its working directory, not `<root>/designs`.
    let tree = Tree::new();
    let home = tree.dir("home");
    let root = tree.dir("home/plain-project");
    let designs = tree.dir("home/plain-project/designs");

    let found = project_root_with_home(&designs.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, root);
    assert_eq!(found.name, "plain-project");
}

#[test]
fn designs_directly_under_home_is_not_lifted_to_home() {
    let tree = Tree::new();
    let home = tree.dir("home");
    let designs = tree.dir("home/designs");

    let found = project_root_with_home(&designs.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, designs, "home itself is never a project");
}

#[test]
fn untitled_document_has_no_root() {
    // No path, no project: the caller keeps its previous behaviour.
    assert_eq!(project_root_for_document(None), None);
    // A path that cannot be a working directory also yields no root.
    assert_eq!(
        project_root_for_document(Some(Path::new("/definitely/not/here/a.op"))),
        None
    );
}

#[test]
fn nonexistent_file_uses_parent() {
    let tree = Tree::new();
    let home = tree.dir("home");
    let project = tree.dir("home/proj");
    fs::write(project.join("Cargo.toml"), "").unwrap();

    // The file is not on disk yet (save-as target), the directory is.
    let found = project_root_with_home(&project.join("new-design.op"), Some(&home)).unwrap();
    assert_eq!(found.root, project);
    assert_eq!(found.source, ProjectRootSource::Marker("Cargo.toml"));

    // A missing parent directory cannot be a working directory.
    assert_eq!(
        project_root_with_home(&project.join("missing/new.op"), Some(&home)),
        None
    );
}

#[test]
fn git_file_marker_for_worktrees_counts() {
    let tree = Tree::new();
    let home = tree.dir("home");
    let worktree = tree.dir("home/wt/feature");
    // A linked worktree's `.git` is a file pointing at the main checkout.
    fs::write(worktree.join(".git"), "gitdir: /elsewhere/.git/worktrees/x").unwrap();
    let docs = tree.dir("home/wt/feature/design");

    let found = project_root_with_home(&docs.join("a.op"), Some(&home)).unwrap();
    assert_eq!(found.root, worktree);
    assert_eq!(found.source, ProjectRootSource::Marker(".git"));
}
