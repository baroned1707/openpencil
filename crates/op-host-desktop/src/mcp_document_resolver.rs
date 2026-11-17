//! The desktop's answer to "which documents are open, and may I edit this
//! one?" for the live MCP server.
//!
//! The editor shows one document at a time: the active one lives in
//! `WidgetHostNative::editor_state`, every other open document is parked in
//! the document registry. The pump reaches a parked document through the host's
//! lens (`with_parked_document`), so an agent can edit a document that is not on
//! screen without touching the one that is.

use crate::disk_baseline_host::DiskBaselines;
use crate::document_registry::DocumentRegistry;
use op_editor_core::{agent_indicators, DocumentId, EditorState};
use op_host_native::WidgetHostNative;
use op_host_services::mcp_live::{
    DocumentListing, DocumentResolver, InPlaceSaveTarget, ResolveError,
};

/// One pump's view of the desktop: the host showing the on-screen document,
/// the registry holding every other open document, and a listing taken at the
/// start of the pump.
pub(crate) struct DesktopDocumentResolver<'a> {
    host: &'a mut WidgetHostNative,
    documents: &'a mut DocumentRegistry,
    listings: Vec<DocumentListing>,
    /// What the editor recorded of each open file, and whether a Cmd+S write is
    /// still running. Absent (a test's bare resolver), the trait default
    /// applies: the file as it is now is the baseline.
    save_context: Option<(&'a DiskBaselines, bool)>,
}

impl<'a> DesktopDocumentResolver<'a> {
    pub(crate) fn new(
        host: &'a mut WidgetHostNative,
        documents: &'a mut DocumentRegistry,
        listings: Vec<DocumentListing>,
    ) -> Self {
        Self {
            host,
            documents,
            listings,
            save_context: None,
        }
    }

    /// Answer in-place save questions from the editor's own records.
    pub(crate) fn with_save_context(
        mut self,
        baselines: &'a DiskBaselines,
        save_in_flight: bool,
    ) -> Self {
        self.save_context = Some((baselines, save_in_flight));
        self
    }
}

/// Whether document `id` is open, and whether it is the one on screen. Pure so
/// the rule is testable without a window.
fn lookup(listings: &[DocumentListing], id: DocumentId) -> Result<bool, ResolveError> {
    listings
        .iter()
        .find(|listing| listing.id == id)
        .map(|listing| listing.active)
        .ok_or(ResolveError::Unknown)
}

impl DocumentResolver for DesktopDocumentResolver<'_> {
    fn documents(&self) -> Vec<DocumentListing> {
        self.listings.clone()
    }

    fn in_place_save_target(&self, id: DocumentId) -> InPlaceSaveTarget {
        let path = self
            .listings
            .iter()
            .find(|listing| listing.id == id)
            .and_then(|listing| listing.path.clone());
        let (baseline, save_in_flight) = match (&self.save_context, path.as_deref()) {
            (Some((baselines, in_flight)), Some(path)) => (baselines.get(path), *in_flight),
            (Some((_, in_flight)), None) => (None, *in_flight),
            (None, Some(path)) => (
                op_host_services::disk_fingerprint::DiskFingerprint::capture(path).ok(),
                false,
            ),
            (None, None) => (None, false),
        };
        InPlaceSaveTarget {
            path,
            baseline,
            save_in_flight,
        }
    }

    fn with_document(
        &mut self,
        id: DocumentId,
        f: &mut dyn FnMut(&mut EditorState, u64),
    ) -> Result<(), ResolveError> {
        let active = lookup(&self.listings, id)?;
        // Indicator calls made by the tool belong to THIS document's scope.
        let _scope = agent_indicators::enter_scope(id.0);
        if active {
            let epoch = self.host.document_epoch();
            f(self.host.editor_state_mut(), epoch);
            return Ok(());
        }
        let index = self.documents.index_of(id).ok_or(ResolveError::Unknown)?;
        let mut parked = self
            .documents
            .take_parked(index)
            .ok_or(ResolveError::NotReachable)?;
        // The lens stands the parked document in for the call and restores the
        // on-screen one; it refuses (no call) while collaboration forbids it.
        let ran = self.host.with_parked_document(&mut parked, |host| {
            let epoch = host.document_epoch();
            f(host.editor_state_mut(), epoch);
        });
        self.documents.restore_parked(index, parked);
        ran.ok_or(ResolveError::NotReachable)
    }
}

#[cfg(test)]
#[path = "mcp_document_resolver_tests.rs"]
mod tests;
