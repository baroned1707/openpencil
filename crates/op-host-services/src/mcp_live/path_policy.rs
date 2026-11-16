//! Which files an agent on the live endpoint may read or write.
//!
//! The live server drives the user's editor, and any local process can reach
//! it, so a tool that takes a path must not become "read or write any file as
//! the user". Every path-bearing argument of every tool call is therefore
//! confined to the ALLOWED ROOTS: the project root of each open document
//! (`project_root_for`: the nearest `.git` / `package.json` / ... folder, never
//! the home directory or `/`), plus the folders the user listed in their own
//! settings. The headless `--mcp` / `--mcp-http` modes and the `op` CLI are the
//! user's own explicit command line and keep unrestricted paths.
//!
//! The rules, in the order they are checked:
//!
//! * the path is absolute (a Finder-launched editor's cwd is `/`);
//! * its deepest EXISTING ancestor, symlinks and any `..` in that part
//!   resolved by the OS, plus the not-yet-existing tail lies inside a root (so
//!   a symlink or a `..` that leaves the root is refused, and a file that does
//!   not exist yet is judged by where it would be created);
//! * the not-yet-existing tail has no `..` (there is nothing to resolve it
//!   against);
//! * a dangling symlink is refused (writing through it would land outside);
//! * a write never targets a hidden folder below the root (`.git`, `.claude`,
//!   `.ssh`, ... where a file is configuration or code), a directory, or a file
//!   some editor tab has open (the next Save would silently undo it).
//!
//! Nothing here creates a directory: a tool only ever creates folders below a
//! path that passed these checks, so nothing is created outside a root.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use op_mcp::ToolCall;

use super::{DocumentListing, McpLiveError};

/// How a tool uses a path argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathUse {
    /// A file the tool reads.
    ReadFile,
    /// A folder the tool lists.
    ReadDir,
    /// A file the tool creates or replaces.
    WriteFile,
    /// A folder the tool puts files into (created when absent).
    WriteDir,
    /// A `.op` file the call edits in place and saves back. Which tab has it
    /// open is `file_path.rs`'s business (it explains how to edit the live
    /// canvas instead), so only the folder rules apply here.
    EditFile,
}

impl PathUse {
    fn writes(self) -> bool {
        matches!(self, Self::WriteFile | Self::WriteDir | Self::EditFile)
    }
}

/// Why a path was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRefusal {
    /// Every open document is unsaved, so there is no folder to allow.
    NoRoots,
    NotAbsolute,
    ParentSegment,
    /// Outside every allowed root (also: a symlink that leaves one).
    Outside,
    /// The path, or a folder above it, is a symlink to nowhere.
    DanglingSymlink,
    /// A write below a hidden folder of the root.
    HiddenFolder,
    /// A write onto an existing folder.
    IsDirectory,
    /// A folder target that is an existing file.
    NotDirectory,
    /// A write onto a file one of the editor's tabs has open.
    OpenInEditor,
}

impl fmt::Display for PathRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoRoots => {
                "every open document is unsaved, so no folder is open to the agent; save a \
                 document first (or call create_document)"
            }
            Self::NotAbsolute => "the path must be absolute",
            Self::ParentSegment => {
                "the part of the path that does not exist yet must not contain '..'"
            }
            Self::Outside => "the path is outside the folders the agent may use",
            Self::DanglingSymlink => "the path goes through a symlink that points nowhere",
            Self::HiddenFolder => {
                "writing below a hidden folder (.git, .claude, ...) is not allowed"
            }
            Self::IsDirectory => "the target is a folder, not a file",
            Self::NotDirectory => "the target exists and is not a folder",
            Self::OpenInEditor => {
                "the file is open in an editor tab; edit that document through its documentId \
                 instead of writing the file behind the editor"
            }
        })
    }
}

/// One path-bearing argument of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathArgument {
    pub(super) key: &'static str,
    pub(super) value: String,
    pub(super) usage: PathUse,
}

/// How `tool` uses the argument `key`, or `None` for a key that is no path.
///
/// Keyed by argument name rather than by tool, so a tool added later that
/// reuses one of these names is confined without anyone remembering to list
/// it. The list is every path-bearing argument the tool implementations read
/// (`op-mcp`, `mcp_serve`, `brand_extract`), snake_case aliases included.
fn classify(tool: &str, key: &str) -> Option<(&'static str, PathUse)> {
    Some(match key {
        // `save_document` writes its snapshot HERE; every other tool edits
        // the named file in place.
        "filePath" if tool == "save_document" => ("filePath", PathUse::WriteFile),
        "filePath" => ("filePath", PathUse::EditFile),
        "sourceFilePath" => ("sourceFilePath", PathUse::ReadFile),
        "svgPath" => ("svgPath", PathUse::ReadFile),
        "svg_path" => ("svg_path", PathUse::ReadFile),
        "htmlPath" => ("htmlPath", PathUse::ReadFile),
        "html_path" => ("html_path", PathUse::ReadFile),
        "snapshotPath" => ("snapshotPath", PathUse::ReadFile),
        "snapshot_path" => ("snapshot_path", PathUse::ReadFile),
        "imagePath" => ("imagePath", PathUse::ReadFile),
        "presetPath" if tool == "save_theme_preset" => ("presetPath", PathUse::WriteFile),
        "presetPath" => ("presetPath", PathUse::ReadFile),
        "outputPath" => ("outputPath", PathUse::WriteFile),
        "outputDir" => ("outputDir", PathUse::WriteDir),
        "directory" => ("directory", PathUse::ReadDir),
        _ => return None,
    })
}

/// Every non-blank path argument of `call`.
///
/// The value is kept EXACTLY as the client wrote it: some tools resolve the
/// untrimmed string (`resolve_path`), so a leading space must not turn a
/// relative path into an absolute-looking one here. Only the blank test and the
/// canvas placeholder look at the trimmed text.
pub(super) fn path_arguments(call: &ToolCall) -> Vec<PathArgument> {
    call.arguments
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .filter_map(|(key, value)| {
            let (key, usage) = classify(&call.tool, key)?;
            // `live://canvas` means "the open document" only as the `filePath`
            // an in-place tool edits, and only spelled EXACTLY like that: any
            // other `live://canvas...` string is a relative path to the tools.
            let canvas_placeholder = usage == PathUse::EditFile
                && crate::mcp_serve::file_path::LIVE_CANVAS_SENTINELS.contains(&value.trim());
            (!canvas_placeholder).then(|| PathArgument {
                key,
                value: value.clone(),
                usage,
            })
        })
        .collect()
}

/// The folders an agent may use: the project root of every open document, and
/// the user's own extra folders. Canonical, without duplicates.
pub(super) fn allowed_roots(open: &[DocumentListing], extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = open
        .iter()
        .filter_map(|listing| listing.path.as_deref())
        .filter_map(crate::project_root::project_root_for)
        .map(|project| project.root)
        .chain(
            extra
                .iter()
                .filter_map(|folder| std::fs::canonicalize(folder).ok())
                .filter(|folder| folder.is_dir()),
        )
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Refuse the call when any of its path arguments leaves the allowed roots.
pub(super) fn vet_call(
    call: &ToolCall,
    open: &[DocumentListing],
    extra_roots: &[PathBuf],
) -> Result<(), McpLiveError> {
    let arguments = path_arguments(call);
    if arguments.is_empty() {
        return Ok(());
    }
    let roots = allowed_roots(open, extra_roots);
    let open_files: Vec<PathBuf> = open
        .iter()
        .filter_map(|listing| listing.path.as_deref())
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
        .collect();
    for argument in arguments {
        if let Err(reason) = confine(&argument, &roots, &open_files) {
            return Err(McpLiveError::PathNotAllowed {
                tool: call.tool.clone(),
                argument: argument.key.to_string(),
                path: argument.value,
                reason,
                roots,
            });
        }
    }
    Ok(())
}

fn confine(
    argument: &PathArgument,
    roots: &[PathBuf],
    open_files: &[PathBuf],
) -> Result<(), PathRefusal> {
    if roots.is_empty() {
        return Err(PathRefusal::NoRoots);
    }
    let path = Path::new(&argument.value);
    if !path.is_absolute() {
        return Err(PathRefusal::NotAbsolute);
    }
    let resolved = resolve_through_existing_ancestor(path)?;
    let root = roots
        .iter()
        .find(|root| resolved.starts_with(root))
        .ok_or(PathRefusal::Outside)?;
    if argument.usage.writes() {
        let below_root = resolved.strip_prefix(root).unwrap_or(&resolved);
        if below_root.components().any(is_hidden) {
            return Err(PathRefusal::HiddenFolder);
        }
    }
    match argument.usage {
        PathUse::WriteFile => {
            if resolved.is_dir() {
                return Err(PathRefusal::IsDirectory);
            }
            if open_files.contains(&resolved) {
                return Err(PathRefusal::OpenInEditor);
            }
        }
        PathUse::EditFile if resolved.is_dir() => return Err(PathRefusal::IsDirectory),
        PathUse::WriteDir if resolved.exists() && !resolved.is_dir() => {
            return Err(PathRefusal::NotDirectory)
        }
        _ => {}
    }
    Ok(())
}

fn is_hidden(component: Component<'_>) -> bool {
    matches!(component, Component::Normal(name) if name.to_string_lossy().starts_with('.'))
}

/// `path` with every symlink of its existing part resolved, followed by the
/// part that does not exist yet.
///
/// Existence is probed with `symlink_metadata`, which does not follow a final
/// symlink: `exists()` reports a dangling link as absent, and the write would
/// then follow it out of the root.
fn resolve_through_existing_ancestor(path: &Path) -> Result<PathBuf, PathRefusal> {
    let mut missing: Vec<&OsStr> = Vec::new();
    let mut cursor = path;
    loop {
        match std::fs::symlink_metadata(cursor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if matches!(cursor.components().next_back(), Some(Component::ParentDir)) {
                    return Err(PathRefusal::ParentSegment);
                }
                missing.push(cursor.file_name().ok_or(PathRefusal::Outside)?);
                cursor = cursor.parent().ok_or(PathRefusal::Outside)?;
            }
            Err(_) => return Err(PathRefusal::Outside),
        }
    }
    let mut resolved = std::fs::canonicalize(cursor).map_err(|_| PathRefusal::DanglingSymlink)?;
    for name in missing.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

#[cfg(test)]
#[path = "path_policy_tests.rs"]
mod tests;
