//! The desktop's list of open documents.
//!
//! The editor shows one document at a time: the one in
//! `WidgetHostNative::editor_state`. Every other open document is parked here
//! as a [`ParkedDocument`] together with the desktop-side state that belongs to
//! it (file path, Git binding, pending collaboration fork saves). This module
//! is plain data and bookkeeping with no window or event loop, so the rules
//! (ids, de-duplication by path, which tab becomes active, what is dirty) are
//! unit-testable on their own. `document_tabs_host` drives it from `DesktopApp`.
//!
//! The on-screen slot holds no parked document (`parked == None`). Its `path`
//! is a mirror that `DesktopApp` refreshes from `current_path` before it asks
//! any question of the registry; its Git session and fork saves live on
//! `DesktopApp` while the slot is active.

use crate::git_session::GitSession;
use op_editor_core::{DocumentId, DocumentTabStatus, DocumentTabView, SessionDocument};
use op_host_native::widget_host::ParkedDocument;
use std::path::{Path, PathBuf};

/// A collaboration Save-As fork: `(epoch, generation, revision, target)`.
pub(crate) type CollabForkSave = (u64, u64, u64, PathBuf);

/// Desktop-side state that follows one document.
#[derive(Default)]
pub(crate) struct SlotLocalState {
    /// File the document is bound to; `None` while untitled.
    pub path: Option<PathBuf>,
    /// The Git repository bound to the document. `None` for the on-screen slot
    /// (the live session is `DesktopApp::git_session`) and for a document that
    /// has not been bound yet.
    pub git_session: Option<GitSession>,
    /// Collaboration fork saves issued from this document.
    pub collab_fork_saves: Vec<CollabForkSave>,
}

/// One open document.
pub(crate) struct DocumentSlot {
    pub id: DocumentId,
    pub local: SlotLocalState,
    /// The document itself while it is not on screen; `None` for the active slot.
    pub parked: Option<ParkedDocument>,
}

impl DocumentSlot {
    /// The folder name shown beside the tab; empty for an untitled document.
    fn project_name(&self) -> String {
        self.local
            .path
            .as_deref()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn label(&self, untitled: &str) -> String {
        self.local
            .path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| untitled.to_owned())
    }
}

/// Which open document a save acknowledgement belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EpochOwner {
    /// The document on screen.
    Active,
    /// A parked document, by position.
    Parked(usize),
    /// No open document has this epoch (it was closed or replaced).
    Unknown,
}

pub(crate) struct DocumentRegistry {
    slots: Vec<DocumentSlot>,
    active: usize,
    next_id: u64,
}

impl DocumentRegistry {
    /// A registry holding the single on-screen document.
    pub fn new() -> Self {
        let mut registry = Self {
            slots: Vec::new(),
            active: 0,
            next_id: 1,
        };
        let first = registry.new_slot();
        registry.slots.push(first);
        registry
    }

    /// A fresh slot with a never-used id and no document parked in it.
    pub fn new_slot(&mut self) -> DocumentSlot {
        let id = DocumentId(self.next_id);
        self.next_id += 1;
        DocumentSlot {
            id,
            local: SlotLocalState::default(),
            parked: None,
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_id(&self) -> DocumentId {
        self.slots[self.active].id
    }

    pub fn index_of(&self, id: DocumentId) -> Option<usize> {
        self.slots.iter().position(|slot| slot.id == id)
    }

    pub fn id_at(&self, index: usize) -> Option<DocumentId> {
        self.slots.get(index).map(|slot| slot.id)
    }

    /// Refresh the on-screen slot's mirrored path.
    pub fn set_active_path(&mut self, path: Option<PathBuf>) {
        self.slots[self.active].local.path = path;
    }

    /// The slot already showing `path`, matched on the canonical path so a
    /// relative path, a symlink or a `..` hop does not open a duplicate tab.
    pub fn index_of_path(&self, path: &Path) -> Option<usize> {
        let wanted = canonical_or_same(path);
        self.slots.iter().position(|slot| {
            slot.local
                .path
                .as_deref()
                .is_some_and(|open| canonical_or_same(open) == wanted)
        })
    }

    /// Take the parked document of a non-active slot, if it has one.
    pub fn take_parked(&mut self, index: usize) -> Option<ParkedDocument> {
        if index == self.active {
            return None;
        }
        self.slots.get_mut(index)?.parked.take()
    }

    /// Put back a document taken with [`Self::take_parked`] (the swap refused).
    pub fn restore_parked(&mut self, index: usize, parked: ParkedDocument) {
        self.slots[index].parked = Some(parked);
    }

    /// Take the desktop-side state of a non-active slot.
    pub fn take_local(&mut self, index: usize) -> SlotLocalState {
        std::mem::take(&mut self.slots[index].local)
    }

    /// Record that `target` is now on screen. The document that was on screen
    /// (`previous`) is parked in the slot that held it.
    pub fn finish_switch(
        &mut self,
        target: usize,
        previous: ParkedDocument,
        previous_local: SlotLocalState,
    ) {
        let leaving = &mut self.slots[self.active];
        leaving.parked = Some(previous);
        leaving.local = previous_local;
        self.active = target;
    }

    /// Append `slot` and show it. The document that was on screen is parked in
    /// the slot that held it. Returns the new slot's index.
    pub fn push_active(
        &mut self,
        previous: ParkedDocument,
        previous_local: SlotLocalState,
        slot: DocumentSlot,
    ) -> usize {
        let leaving = &mut self.slots[self.active];
        leaving.parked = Some(previous);
        leaving.local = previous_local;
        self.slots.push(slot);
        self.active = self.slots.len() - 1;
        self.active
    }

    /// Remove a slot that is not on screen and return it. Positions after it
    /// shift down; the active index follows its slot.
    pub fn remove(&mut self, index: usize) -> Option<DocumentSlot> {
        if index == self.active || index >= self.slots.len() {
            return None;
        }
        let removed = self.slots.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        Some(removed)
    }

    /// Whether the document at `index` has unsaved edits. `live_dirty` answers
    /// for the on-screen document, which this registry does not hold.
    pub fn is_dirty(&self, index: usize, live_dirty: bool) -> bool {
        if index == self.active {
            return live_dirty;
        }
        self.slots
            .get(index)
            .and_then(|slot| slot.parked.as_ref())
            .is_some_and(ParkedDocument::is_dirty)
    }

    /// Positions of every document with unsaved edits, in tab order.
    pub fn dirty_indices(&self, live_dirty: bool) -> Vec<usize> {
        (0..self.slots.len())
            .filter(|&index| self.is_dirty(index, live_dirty))
            .collect()
    }

    /// Which document a save acknowledgement for `epoch` belongs to.
    pub fn owner_of_epoch(&self, epoch: u64, live_epoch: u64) -> EpochOwner {
        if epoch == live_epoch {
            return EpochOwner::Active;
        }
        self.slots
            .iter()
            .position(|slot| slot.parked.as_ref().is_some_and(|p| p.epoch() == epoch))
            .map_or(EpochOwner::Unknown, EpochOwner::Parked)
    }

    pub fn slot_mut(&mut self, index: usize) -> Option<&mut DocumentSlot> {
        self.slots.get_mut(index)
    }

    /// The tab rows the strip draws.
    pub fn tab_views(&self, live_dirty: bool, untitled_label: &str) -> Vec<DocumentTabView> {
        self.slots
            .iter()
            .enumerate()
            .map(|(index, slot)| DocumentTabView {
                id: slot.id,
                label: slot.label(untitled_label),
                project: slot.project_name(),
                dirty: self.is_dirty(index, live_dirty),
                status: DocumentTabStatus::Idle,
                active: index == self.active,
            })
            .collect()
    }

    /// Every file open in some tab, on screen or parked.
    pub fn open_paths(&self) -> Vec<PathBuf> {
        self.slots
            .iter()
            .filter_map(|slot| slot.local.path.clone())
            .collect()
    }

    /// The documents to reopen on the next launch: those bound to a file.
    pub fn session_documents(&self) -> Vec<SessionDocument> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                slot.local.path.as_ref().map(|path| SessionDocument {
                    path: path.to_string_lossy().into_owned(),
                    active: index == self.active,
                })
            })
            .collect()
    }
}

fn canonical_or_same(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What is running that must finish before documents may be switched.
///
/// Agent runs, imports and background writers are not yet bound to the
/// document that started them, so switching under one would deliver its result
/// to whichever document is on screen then.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockReason {
    ChatTurn,
    DesignRun,
    SubAgents,
    Codegen,
    DesignMd,
    Import,
    GitJob,
    BackgroundSave,
    Collaboration,
}

impl std::fmt::Display for BlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ChatTurn => "a chat turn is running",
            Self::DesignRun => "a design run is in progress",
            Self::SubAgents => "sub-agents are running",
            Self::Codegen => "code generation is running",
            Self::DesignMd => "design-system generation is running",
            Self::Import => "an import is in progress",
            Self::GitJob => "a Git pull, push or clone is running",
            Self::BackgroundSave => "a save is being written",
            Self::Collaboration => "a collaboration session is active",
        })
    }
}

/// The running work the desktop observed this frame.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ActivityFlags {
    pub chat_turn: bool,
    pub design_run: bool,
    pub sub_agents: bool,
    pub codegen: bool,
    pub design_md: bool,
    pub import: bool,
    pub git_job: bool,
    pub background_save: bool,
    pub collaboration: bool,
}

impl ActivityFlags {
    /// The first thing that forbids switching, in the order a user would
    /// recognize it. `None` when the editor is free to switch.
    pub fn blocked_by(&self) -> Option<BlockReason> {
        [
            (self.collaboration, BlockReason::Collaboration),
            (self.chat_turn, BlockReason::ChatTurn),
            (self.design_run, BlockReason::DesignRun),
            (self.sub_agents, BlockReason::SubAgents),
            (self.codegen, BlockReason::Codegen),
            (self.design_md, BlockReason::DesignMd),
            (self.import, BlockReason::Import),
            (self.git_job, BlockReason::GitJob),
            (self.background_save, BlockReason::BackgroundSave),
        ]
        .into_iter()
        .find_map(|(active, reason)| active.then_some(reason))
    }
}

#[cfg(test)]
#[path = "document_registry_tests.rs"]
mod tests;
