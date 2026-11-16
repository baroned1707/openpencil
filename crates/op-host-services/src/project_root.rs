//! Project-root resolution for a saved document.
//!
//! Agent subprocesses (Claude Code, Codex, ACP agents) look for the project's
//! `.claude/`, `CLAUDE.md`, `.mcp.json` and relative paths in their working
//! directory. A Finder-launched desktop app runs with cwd `/`, so the agent
//! must be pointed at the project that owns the document instead. This module
//! answers "which directory is that?" from the document's file path alone.
//!
//! The rule is the usual editor one: walk up from the file's directory to the
//! nearest ancestor holding a project marker. The walk never reaches the
//! user's home directory or the filesystem root (home often carries a global
//! `~/.claude`, which would otherwise make every document a "project" rooted
//! at `~`). With no marker the file's own directory is the root.

use std::path::{Path, PathBuf};

/// Entries whose presence marks a directory as a project root. `.git` may be a
/// directory or a file (worktrees and submodules use a `.git` file), so every
/// marker is tested with `exists()`.
const PROJECT_MARKERS: [&str; 6] = [
    ".git",
    ".mcp.json",
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    ".claude",
];

/// How a [`ProjectRoot`] was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRootSource {
    /// An ancestor directory held this project marker.
    Marker(&'static str),
    /// No ancestor held a marker; the document's own directory is the root.
    FileDir,
}

/// The directory an agent should treat as the project for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    /// Canonical absolute path of the project directory (always exists).
    pub root: PathBuf,
    /// The directory's own name, for grouping documents by project.
    pub name: String,
    pub source: ProjectRootSource,
}

/// Resolve the project root of the document stored at `path`.
///
/// `path` need not exist (a document about to be saved still has a home
/// directory), but its parent directory must. Returns `None` when there is no
/// usable directory: the parent does not exist, or the document sits directly
/// in the user's home directory or the filesystem root, which are never
/// treated as a project. Callers keep their previous behaviour on `None`.
pub fn project_root_for(path: &Path) -> Option<ProjectRoot> {
    project_root_with_home(path, canonical_home().as_deref())
}

/// The user's home directory in canonical form, or `None` when it cannot be
/// determined. Canonical so it compares equal to a canonicalized candidate.
pub(crate) fn canonical_home() -> Option<PathBuf> {
    dirs::home_dir().and_then(|home| std::fs::canonicalize(home).ok())
}

/// Project root of the open document, or `None` for an unsaved (untitled)
/// document, which has no path to resolve from; callers keep their previous
/// behaviour then. This is the entry point for `DesktopApp::current_path`.
pub fn project_root_for_document(path: Option<&Path>) -> Option<ProjectRoot> {
    path.and_then(project_root_for)
}

/// [`project_root_for`] with the home directory injected, so tests do not
/// depend on the machine's real home. `home` must already be canonical.
fn project_root_with_home(path: &Path, home: Option<&Path>) -> Option<ProjectRoot> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let start = std::fs::canonicalize(parent).ok()?;
    if !start.is_dir() {
        return None;
    }
    // Home and `/` are never projects, whatever they happen to contain, so a
    // document sitting directly in one of them has no project directory.
    if is_forbidden_root(&start, home) {
        return None;
    }
    for ancestor in start.ancestors() {
        // Checked before the markers so `~/.claude` cannot turn home into a
        // project; the walk simply ends at the first forbidden directory.
        if is_forbidden_root(ancestor, home) {
            break;
        }
        if let Some(marker) = PROJECT_MARKERS
            .iter()
            .copied()
            .find(|marker| ancestor.join(marker).exists())
        {
            return Some(project_root(ancestor, ProjectRootSource::Marker(marker)));
        }
    }
    // No marker: a file inside a `designs/` folder belongs to the project that
    // contains that folder (the layout `create_document` writes), so the agent
    // keeps the project root it was started in rather than `<root>/designs`.
    if start.file_name().is_some_and(|name| name == "designs") {
        if let Some(root) = start.parent() {
            if !is_forbidden_root(root, home) {
                return Some(project_root(root, ProjectRootSource::FileDir));
            }
        }
    }
    Some(project_root(&start, ProjectRootSource::FileDir))
}

/// Whether `dir` (canonical) may never be a project: the filesystem root or
/// the user's home directory itself. `home` must already be canonical.
pub(crate) fn is_forbidden_root(dir: &Path, home: Option<&Path>) -> bool {
    dir.parent().is_none() || home == Some(dir)
}

fn project_root(dir: &Path, source: ProjectRootSource) -> ProjectRoot {
    ProjectRoot {
        root: dir.to_path_buf(),
        name: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        source,
    }
}

#[cfg(test)]
#[path = "project_root_tests.rs"]
mod tests;
