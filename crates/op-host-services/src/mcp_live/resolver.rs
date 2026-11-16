//! What the live MCP server needs from the host to reach a document.
//!
//! The server never holds an `EditorState`; the UI thread lends one to each
//! pump. A host that keeps several documents open implements
//! [`DocumentResolver`] over them, so a tool call that names a document by id
//! reaches exactly that document. Hosts with one document (every headless
//! mode, tests) use [`SingleDocumentResolver`].

use std::path::PathBuf;

use op_editor_core::{DocumentId, DocumentTabStatus, EditorState};

pub use crate::mcp_serve::document_listing::DocumentListing;

/// Why a resolver could not lend a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// No open document has this id (it was closed, or never existed).
    Unknown,
    /// The document is open but the host cannot lend its state right now
    /// (today: it is parked behind the document on screen).
    NotReachable,
}

/// The host's open documents, as the UI thread sees them.
pub trait DocumentResolver {
    /// Every open document, in tab order.
    fn documents(&self) -> Vec<DocumentListing>;

    /// Run `f` on the state of document `id` together with that document's
    /// host epoch (the counter the host bumps whenever the document is
    /// replaced wholesale). `f` runs at most once, and not at all on error.
    fn with_document(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<(), ResolveError>;

    /// The document on screen.
    fn active_document(&self) -> Option<DocumentId> {
        self.documents()
            .into_iter()
            .find(|listing| listing.active)
            .map(|listing| listing.id)
    }
}

/// A host with exactly one document, `d1`, which is always the active one.
pub struct SingleDocumentResolver<'a> {
    pub state: &'a mut EditorState,
    /// The host's epoch for the document (`0` when the host has none).
    pub host_epoch: u64,
    /// The file the document is bound to; `None` while untitled.
    pub path: Option<PathBuf>,
}

impl<'a> SingleDocumentResolver<'a> {
    pub fn new(state: &'a mut EditorState) -> Self {
        Self {
            state,
            host_epoch: 0,
            path: None,
        }
    }
}

impl DocumentResolver for SingleDocumentResolver<'_> {
    fn documents(&self) -> Vec<DocumentListing> {
        let label = self
            .path
            .as_deref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        vec![DocumentListing {
            id: crate::mcp_serve::document_address::HEADLESS_DOCUMENT,
            label,
            path: self.path.clone(),
            project: crate::mcp_serve::document_listing::project_name_for(self.path.as_deref()),
            dirty: self.state.is_dirty(),
            active: true,
            status: DocumentTabStatus::Idle,
            nonce: None,
        }]
    }

    fn with_document(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<(), ResolveError> {
        if id != crate::mcp_serve::document_address::HEADLESS_DOCUMENT {
            return Err(ResolveError::Unknown);
        }
        f(self.state, self.host_epoch);
        Ok(())
    }
}
