//! Folders the user opened as projects, and the rule that ties a document to
//! one of them.
//!
//! [`project_root_for`](super::project_root_for) infers a project from markers
//! (`.git`, `package.json`, ...). A folder the user picked explicitly is a
//! project whether or not it carries one, and it wins over inference: a
//! document inside it belongs to the DEEPEST such folder, so the tab strip,
//! the Projects dock and the agent's working directory all agree.
//!
//! [`ExplicitRoots`] is the one set every caller asks. It is an owned handle,
//! not a global: the app and each test hold their own, so two of them never
//! see each other's folders. Roots are stored canonical; the canonical parent
//! of each asked-about document is memoized, so a frame never touches the disk
//! after the first look at a path.
//!
//! [`check_project_root`] decides which folders may be opened at all: never
//! the filesystem root, the home directory (or a folder holding it), or a
//! system directory.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

/// Canonical parents remembered before the memo starts over.
const MEMO_LIMIT: usize = 1024;

/// The explicitly opened project folders. See the module docs.
#[derive(Default)]
pub struct ExplicitRoots {
    roots: RwLock<Vec<PathBuf>>,
    /// Bumped on every change, so a caller that memoized an answer can tell
    /// it went stale.
    generation: AtomicU64,
    /// `document path -> canonical parent directory` (`None` = unresolvable).
    parents: Mutex<HashMap<PathBuf, Option<PathBuf>>>,
}

impl ExplicitRoots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Changes whenever a root is added or removed.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// The roots, sorted.
    pub fn snapshot(&self) -> Vec<PathBuf> {
        self.read().clone()
    }

    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    pub fn contains(&self, root: &Path) -> bool {
        self.read().iter().any(|known| known == root)
    }

    /// Add a canonical `root`. Returns whether it was new.
    pub fn insert(&self, root: PathBuf) -> bool {
        let mut roots = self.write();
        if roots.contains(&root) {
            return false;
        }
        roots.push(root);
        roots.sort();
        self.generation.fetch_add(1, Ordering::AcqRel);
        true
    }

    /// Forget `root`. Returns whether it was there.
    pub fn remove(&self, root: &Path) -> bool {
        let mut roots = self.write();
        let before = roots.len();
        roots.retain(|known| known != root);
        let removed = roots.len() != before;
        if removed {
            self.generation.fetch_add(1, Ordering::AcqRel);
        }
        removed
    }

    /// The deepest explicit root containing the document at `path`, or `None`
    /// when no root does (the caller then falls back to inference). Reads the
    /// disk once per path.
    pub fn root_for(&self, path: &Path) -> Option<PathBuf> {
        if self.is_empty() {
            return None;
        }
        let parent = self.canonical_parent(path)?;
        deepest_containing(&self.read(), &parent).cloned()
    }

    /// [`Self::root_for`] for a directory that is already canonical.
    pub fn root_for_dir(&self, canonical_dir: &Path) -> Option<PathBuf> {
        deepest_containing(&self.read(), canonical_dir).cloned()
    }

    fn canonical_parent(&self, path: &Path) -> Option<PathBuf> {
        if let Some(known) = self
            .parents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(path)
        {
            return known.clone();
        }
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let resolved = std::fs::canonicalize(parent).ok();
        let mut memo = self
            .parents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if memo.len() >= MEMO_LIMIT {
            memo.clear();
        }
        memo.insert(path.to_path_buf(), resolved.clone());
        resolved
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<PathBuf>> {
        self.roots
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Vec<PathBuf>> {
        self.roots
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The deepest of `roots` that is `dir` or an ancestor of it. Pure.
pub fn deepest_containing<'a>(roots: &'a [PathBuf], dir: &Path) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|root| dir.starts_with(root))
        .max_by_key(|root| root.components().count())
}

/// Why a folder may not be opened as a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsafeRoot {
    /// The filesystem root.
    FilesystemRoot,
    /// The home directory, or a folder that holds it (`/Users`).
    HomeOrAncestor,
    /// A system directory.
    SystemDirectory,
}

impl fmt::Display for UnsafeRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::FilesystemRoot => "the filesystem root",
            Self::HomeOrAncestor => "the home directory",
            Self::SystemDirectory => "a system directory",
        })
    }
}

impl std::error::Error for UnsafeRoot {}

/// System folders a project may not be, in canonical form.
#[derive(Debug, Clone, Default)]
pub struct SystemDirs {
    /// Refused as themselves only: their children are ordinary places (a
    /// temp directory lives in `/private/var/folders`, a volume under
    /// `/Volumes`).
    pub exact: Vec<PathBuf>,
    /// Refused together with everything inside them.
    pub trees: Vec<PathBuf>,
}

impl SystemDirs {
    /// This machine's system folders. Each is canonicalized, so `/etc` is
    /// recognised as `/private/etc`; ones that do not exist are left out.
    pub fn host() -> Self {
        let canonical = |names: &[&str]| -> Vec<PathBuf> {
            names
                .iter()
                .filter_map(|name| std::fs::canonicalize(name).ok())
                .collect()
        };
        Self {
            exact: canonical(&[
                "/",
                "/private",
                "/var",
                "/tmp",
                "/Volumes",
                "/Applications",
                "/Users",
                "/home",
                "/opt",
                "/mnt",
                "/media",
            ]),
            trees: canonical(&["/System", "/Library", "/usr", "/bin", "/sbin", "/etc"]),
        }
    }
}

/// Whether `candidate` (canonical) may be opened as a project. `home` is the
/// canonical home directory when known.
pub fn check_project_root(
    candidate: &Path,
    home: Option<&Path>,
    system: &SystemDirs,
) -> Result<(), UnsafeRoot> {
    if candidate.parent().is_none() {
        return Err(UnsafeRoot::FilesystemRoot);
    }
    if home.is_some_and(|home| home.starts_with(candidate)) {
        return Err(UnsafeRoot::HomeOrAncestor);
    }
    let exact = system.exact.iter().any(|dir| dir == candidate);
    let inside = system.trees.iter().any(|dir| candidate.starts_with(dir));
    if exact || inside {
        return Err(UnsafeRoot::SystemDirectory);
    }
    Ok(())
}

#[cfg(test)]
#[path = "project_root_explicit_tests.rs"]
mod tests;
