//! Open-document tabs — the plain-data contract between the document
//! registry (host) and the tab strip widget (UI).
//!
//! The host owns the registry of open documents and mirrors it into
//! [`DocumentTabView`] rows on `EditorUiState.document_tabs`; the widget only
//! reads those rows and records what the user asked for in
//! `EditorUiState.pending_document_request`, which the host drains. Neither
//! side reaches into the other's types, and everything here is wasm32-clean.

/// Stable id of one open document for the lifetime of the process. Never
/// reused, so a stale request can be detected instead of hitting another tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

/// What an agent is doing in a document, shown as a dot on its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DocumentTabStatus {
    #[default]
    Idle,
    Running,
    NeedsReview,
    Done,
    Error,
}

/// One tab as the widget draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentTabView {
    pub id: DocumentId,
    /// File name, or the untitled placeholder.
    pub label: String,
    /// Project name used to group tabs; empty when unknown.
    pub project: String,
    pub dirty: bool,
    pub status: DocumentTabStatus,
    pub active: bool,
}

/// A user request raised by the tab strip, executed by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentTabRequest {
    Switch(DocumentId),
    Close(DocumentId),
    /// Switch by 0-based position (Cmd+1..9).
    SwitchToIndex(usize),
}
