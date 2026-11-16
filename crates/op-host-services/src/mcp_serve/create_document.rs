//! The `create_document` MCP tool: give an untitled live document its first
//! file, at `<projectRoot>/designs/<name>.op`.
//!
//! This module is the host-free half: argument validation, resolving the
//! target path safely, and writing the file without ever replacing one. The
//! live desktop server (`mcp_live::create_document`) drives it on the UI
//! thread and tells the editor to bind the file; the headless transports have
//! a backing file already and refuse. Nothing here touches editor state except
//! reading the document it is asked to write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use op_editor_core::EditorState;
use op_mcp::{McpTool, ToolErrorCode, ToolOutcome};

pub use super::create_document_error::{CreateDocumentError, NameProblem};
use super::tool_profile::McpAccessProfile;
use crate::doc_io::{save_new_to_path, SaveNewError};
use crate::project_root;

pub const TOOL_NAME: &str = "create_document";
const DESIGNS_DIR: &str = "designs";
const EXTENSION: &str = ".op";
const MAX_NAME_CHARS: usize = 100;

/// Everything the write needs, resolved and checked by [`resolve_target`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTarget {
    /// Canonical project folder.
    pub project_root: PathBuf,
    /// `<project_root>/designs`, canonical when it already exists.
    pub designs_dir: PathBuf,
    /// `<designs_dir>/<name>.op`; does not exist.
    pub file_path: PathBuf,
    /// The file stem, without `.op`.
    pub name: String,
    designs_existed: bool,
}

/// The two required arguments, as the agent wrote them.
pub fn extract_args(
    arguments: &BTreeMap<String, String>,
) -> Result<(String, String), CreateDocumentError> {
    let project_root = arguments
        .get("projectRoot")
        .filter(|value| !value.trim().is_empty())
        .ok_or(CreateDocumentError::MissingArgument("projectRoot"))?;
    let name = arguments
        .get("name")
        .ok_or(CreateDocumentError::MissingArgument("name"))?;
    Ok((project_root.clone(), name.clone()))
}

/// The file stem for `raw`: the name with one optional trailing `.op` taken
/// off, after checking it is a plain, single path component.
pub fn validate_name(raw: &str) -> Result<&str, NameProblem> {
    if raw.trim().is_empty() {
        return Err(NameProblem::Empty);
    }
    if raw.contains(['/', '\\']) {
        return Err(NameProblem::PathSeparator);
    }
    if raw.contains("..") {
        return Err(NameProblem::ParentReference);
    }
    if raw.chars().any(char::is_control) {
        return Err(NameProblem::ControlCharacter);
    }
    if raw.starts_with('.') {
        return Err(NameProblem::LeadingDot);
    }
    let stem = raw.strip_suffix(EXTENSION).unwrap_or(raw);
    if stem.trim() != stem {
        return Err(NameProblem::EdgeWhitespace);
    }
    if stem.chars().count() > MAX_NAME_CHARS {
        return Err(NameProblem::TooLong {
            max: MAX_NAME_CHARS,
        });
    }
    Ok(stem)
}

/// Check `project_root` and `name` against the filesystem and return where the
/// file would go. Read-only: nothing is created. `home` is the canonical home
/// directory (injected so tests never touch the real one).
pub fn resolve_target(
    project_root: &str,
    name: &str,
    home: Option<&Path>,
) -> Result<CreateTarget, CreateDocumentError> {
    let stem =
        validate_name(name).map_err(|problem| CreateDocumentError::InvalidName { problem })?;
    if !Path::new(project_root).is_absolute() {
        return Err(CreateDocumentError::ProjectRootNotAbsolute {
            path: project_root.to_string(),
        });
    }
    let root = std::fs::canonicalize(project_root).map_err(|error| {
        CreateDocumentError::ProjectRootUnreadable {
            path: project_root.to_string(),
            detail: error.to_string(),
        }
    })?;
    if !root.is_dir() {
        return Err(CreateDocumentError::ProjectRootNotDirectory { path: root });
    }
    if project_root::is_forbidden_root(&root, home) {
        return Err(CreateDocumentError::ProjectRootForbidden { path: root });
    }
    let designs = root.join(DESIGNS_DIR);
    let designs_existed = match std::fs::symlink_metadata(&designs) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(CreateDocumentError::Io {
                action: "inspect",
                path: designs,
                detail: error.to_string(),
            })
        }
    };
    let designs_dir = if designs_existed {
        contained_directory(&designs, &root)?
    } else {
        designs
    };
    let file_path = designs_dir.join(format!("{stem}{EXTENSION}"));
    // `symlink_metadata`, not `exists`: a dangling symlink is an entry too.
    if std::fs::symlink_metadata(&file_path).is_ok() {
        return Err(CreateDocumentError::FileExists { path: file_path });
    }
    Ok(CreateTarget {
        project_root: root,
        designs_dir,
        file_path,
        name: stem.to_string(),
        designs_existed,
    })
}

/// `designs` resolved to its canonical form, required to be a directory inside
/// `root` (a symlink to another folder of the project is fine; one that leaves
/// the project, or points nowhere, is not).
fn contained_directory(designs: &Path, root: &Path) -> Result<PathBuf, CreateDocumentError> {
    let escapes = || CreateDocumentError::DesignsEscapesProject {
        designs: designs.to_path_buf(),
        project_root: root.to_path_buf(),
    };
    let canonical = std::fs::canonicalize(designs).map_err(|_| escapes())?;
    if !canonical.starts_with(root) {
        return Err(escapes());
    }
    if !canonical.is_dir() {
        return Err(CreateDocumentError::DesignsNotDirectory {
            path: designs.to_path_buf(),
        });
    }
    Ok(canonical)
}

/// Create `designs/` if needed and write `state`'s document to the target,
/// never replacing an existing file. On failure nothing is left behind: the
/// temp file is removed and a `designs/` this call created is removed again
/// when empty.
pub fn write_new_document(
    state: &EditorState,
    target: &CreateTarget,
) -> Result<(), CreateDocumentError> {
    if !target.designs_existed {
        std::fs::create_dir_all(&target.designs_dir).map_err(|error| CreateDocumentError::Io {
            action: "create",
            path: target.designs_dir.clone(),
            detail: error.to_string(),
        })?;
    }
    let result = contained_directory(&target.designs_dir, &target.project_root).and_then(|_| {
        save_new_to_path(state, &target.file_path).map_err(|error| match error {
            SaveNewError::AlreadyExists => CreateDocumentError::FileExists {
                path: target.file_path.clone(),
            },
            SaveNewError::Io(error) => CreateDocumentError::Io {
                action: "write",
                path: target.file_path.clone(),
                detail: error.to_string(),
            },
        })
    });
    if result.is_err() && !target.designs_existed {
        // Only succeeds while empty, so it can never remove someone's files.
        let _ = std::fs::remove_dir(&target.designs_dir);
    }
    result
}

/// The registry entry for hosts that list the tool but have no editor window
/// to bind a file to. Live and headless transports answer the call before it
/// reaches the registry.
pub struct CreateDocument;

pub fn create_document_snapshot() -> CreateDocument {
    CreateDocument
}

impl McpTool for CreateDocument {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        ToolOutcome::Err(
            ToolErrorCode::ToolFailed,
            CreateDocumentError::NotLiveEditor.to_string(),
        )
    }
}

/// Headless transports (`--mcp`, `--mcp-http`) serve a document that always
/// has a file, so a `create_document` call there is answered with the same
/// "already saved" refusal the live editor gives. `None` when the line is not
/// a `create_document` call, or when `profile` refuses the tool (the normal
/// dispatch then produces that refusal).
pub fn refuse_in_headless(
    line: &str,
    backing_file: &Path,
    profile: McpAccessProfile,
) -> Option<String> {
    let call = op_mcp::parse_tool_call(line.trim())?;
    if call.tool != TOOL_NAME || profile.refuse_call(&call.tool, &call.arguments).is_some() {
        return None;
    }
    let error = CreateDocumentError::AlreadySaved {
        path: backing_file.to_path_buf(),
    };
    Some(error_response(call.id, &error))
}

/// The `isError` tool reply for `error`.
pub fn error_response(id: op_mcp::RequestId, error: &CreateDocumentError) -> String {
    op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Err {
        id,
        code: error.code(),
        message: error.to_string(),
    })
}

#[cfg(test)]
#[path = "create_document_tests.rs"]
mod tests;
