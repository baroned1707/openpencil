//! One serial queue per document.
//!
//! The server used to serialize every stateful call behind ONE lock, so an
//! agent working in one document made every other agent wait. Each document
//! now has its own gate: calls to different documents run at the same time,
//! calls to one document run one after another. The gate also owns that
//! document's [`OperationLog`], so "check the log, run the call, record the
//! response" happens under the lock a retry would have to wait for.

use std::collections::HashMap;

use super::*;
use operation_log::OperationLog;

/// A document's gate: holding its guard is holding the document's queue slot.
pub(super) type DocumentGate = Mutex<OperationLog>;

#[derive(Default)]
pub(super) struct LiveDocuments {
    gates: Mutex<HashMap<DocumentId, Arc<DocumentGate>>>,
    /// Serializes calls that edit a `.op` file on disk (`filePath`), which
    /// belong to no open document but are still read-modify-write.
    files: Mutex<()>,
}

impl LiveDocuments {
    /// The gate for `id`, created on first use.
    pub(super) fn gate(&self, id: DocumentId) -> Arc<DocumentGate> {
        let mut gates = self.gates.lock().unwrap_or_else(|p| p.into_inner());
        Arc::clone(gates.entry(id).or_default())
    }

    /// Drop the gate of a document that turned out not to exist, so a client
    /// probing made-up ids cannot grow the map.
    pub(super) fn forget(&self, id: DocumentId) {
        self.gates
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
    }

    #[cfg(test)]
    pub(super) fn gate_count(&self) -> usize {
        self.gates.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub(super) fn lock_files(&self) -> std::sync::MutexGuard<'_, ()> {
        self.files.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Lock a gate, tolerating a poisoned mutex (a panicked sibling connection)
/// rather than cascading the panic.
pub(super) fn lock_gate(gate: &DocumentGate) -> std::sync::MutexGuard<'_, OperationLog> {
    gate.lock().unwrap_or_else(|p| p.into_inner())
}
