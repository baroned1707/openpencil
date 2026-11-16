//! Typed refusals for the `create_document` MCP tool (see
//! [`super::create_document`]).
//!
//! Every variant is a tool-level answer the agent reads as an `isError`
//! result, never a transport fault. Each carries structured fields and
//! `Display` words the sentence the agent sees, so the next step (pick another
//! name, pass an absolute folder, use `save_document`) is in the message.

use std::fmt;
use std::path::PathBuf;

/// What is wrong with a document `name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    PathSeparator,
    ParentReference,
    ControlCharacter,
    LeadingDot,
    EdgeWhitespace,
    TooLong { max: usize },
}

impl fmt::Display for NameProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("it is empty"),
            Self::PathSeparator => f.write_str("it contains a path separator"),
            Self::ParentReference => f.write_str("it contains '..'"),
            Self::ControlCharacter => f.write_str("it contains a control character"),
            Self::LeadingDot => f.write_str("it starts with '.'"),
            Self::EdgeWhitespace => f.write_str("it starts or ends with a space"),
            Self::TooLong { max } => write!(f, "it is longer than {max} characters"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateDocumentError {
    /// The live document already has a file.
    AlreadySaved {
        path: PathBuf,
    },
    /// A collaboration session is bound to the document; binding a new file
    /// to it would detach it from the session.
    CollaborationActive,
    /// A required argument was not passed.
    MissingArgument(&'static str),
    ProjectRootNotAbsolute {
        path: String,
    },
    /// The folder does not exist or cannot be read.
    ProjectRootUnreadable {
        path: String,
        detail: String,
    },
    ProjectRootNotDirectory {
        path: PathBuf,
    },
    /// The filesystem root or the user's home directory itself.
    ProjectRootForbidden {
        path: PathBuf,
    },
    InvalidName {
        problem: NameProblem,
    },
    /// `<projectRoot>/designs` exists but is not a directory.
    DesignsNotDirectory {
        path: PathBuf,
    },
    /// `<projectRoot>/designs` resolves outside the project folder.
    DesignsEscapesProject {
        designs: PathBuf,
        project_root: PathBuf,
    },
    /// The target file exists already; it is never overwritten.
    FileExists {
        path: PathBuf,
    },
    /// A filesystem step failed. `action` names it ("create", "write").
    Io {
        action: &'static str,
        path: PathBuf,
        detail: String,
    },
    /// The server has no editor window to bind a new file to (a host that
    /// registers the tool without handling it).
    NotLiveEditor,
}

impl CreateDocumentError {
    /// The wire error class: a missing argument and a malformed one are
    /// argument errors; everything else is the tool declining to act.
    pub fn code(&self) -> op_mcp::ToolErrorCode {
        match self {
            Self::MissingArgument(_) => op_mcp::ToolErrorCode::MissingArgument,
            Self::ProjectRootNotAbsolute { .. }
            | Self::ProjectRootUnreadable { .. }
            | Self::ProjectRootNotDirectory { .. }
            | Self::ProjectRootForbidden { .. }
            | Self::InvalidName { .. } => op_mcp::ToolErrorCode::InvalidArgument,
            _ => op_mcp::ToolErrorCode::ToolFailed,
        }
    }
}

impl fmt::Display for CreateDocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadySaved { path } => write!(
                f,
                "the open document is already saved at {}; use save_document with a filePath to save a copy elsewhere",
                path.display()
            ),
            Self::CollaborationActive => f.write_str(
                "the open document belongs to a collaboration session; leave the session before creating a file for it",
            ),
            Self::MissingArgument(name) => write!(f, "{name} is required"),
            Self::ProjectRootNotAbsolute { path } => write!(
                f,
                "projectRoot must be an absolute path (got '{path}'); pass your own current working directory"
            ),
            Self::ProjectRootUnreadable { path, detail } => {
                write!(f, "projectRoot {path} does not exist or cannot be read: {detail}")
            }
            Self::ProjectRootNotDirectory { path } => {
                write!(f, "projectRoot {} is not a directory", path.display())
            }
            Self::ProjectRootForbidden { path } => write!(
                f,
                "projectRoot {} is the filesystem root or the home directory itself; pass the project folder under it",
                path.display()
            ),
            Self::InvalidName { problem } => write!(
                f,
                "name is not a usable file name: {problem}; use letters, digits, spaces, dashes or underscores"
            ),
            Self::DesignsNotDirectory { path } => {
                write!(f, "{} exists but is not a directory", path.display())
            }
            Self::DesignsEscapesProject {
                designs,
                project_root,
            } => write!(
                f,
                "{} resolves outside the project folder {}; refusing to write there",
                designs.display(),
                project_root.display()
            ),
            Self::FileExists { path } => write!(
                f,
                "{} already exists and is never overwritten; pick another name, or open it with open_document filePath",
                path.display()
            ),
            Self::Io {
                action,
                path,
                detail,
            } => write!(f, "could not {action} {}: {detail}", path.display()),
            Self::NotLiveEditor => f.write_str(
                "create_document needs the live desktop editor with an untitled document; this server has no editor window to bind a new file to",
            ),
        }
    }
}

impl std::error::Error for CreateDocumentError {}
