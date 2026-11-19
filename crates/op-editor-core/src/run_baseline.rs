//! The active page as it stood before an agent run started.
//!
//! A host takes one when it launches a design run and hands it to the loop
//! finalizer, which compares it with the finished document to learn which
//! sections the run changed (`op_orchestrator::run_scope`). It lives here, not
//! in the orchestrator, so the chat session types every host shares can carry
//! it without depending on the design pipeline.

use crate::state::EditorState;
use jian_ops_schema::node::PenNode;

/// The active page's top-level forest at the moment a run started.
///
/// An owned clone: it has to survive the run's own edits, and a run launches
/// far less often than a document is edited.
#[derive(Debug, Clone)]
pub struct RunBaseline {
    page_index: usize,
    roots: Vec<PenNode>,
}

impl RunBaseline {
    /// Capture the active page of `state`.
    pub fn capture(state: &EditorState) -> Self {
        Self {
            page_index: state.ui.active_page_index,
            roots: state.active_children().to_vec(),
        }
    }

    /// Index of the page the baseline was taken from.
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    /// The page's top-level nodes as they were.
    pub fn roots(&self) -> &[PenNode] {
        &self.roots
    }
}
