//! Switching between open documents.
//!
//! The host keeps the document ON SCREEN in `editor_state`, so the ~1,650
//! `editor_state()` call sites never learn that several documents exist. Every
//! other open document is parked in a [`ParkedDocument`]. Switching exchanges
//! the two through [`WidgetHostNative::swap_document`], the one seam that
//! moves a document in or out of the host.
//!
//! What travels with a document:
//!   - its editor data (`DocumentScope`: node tree, selection, history,
//!     revision counters, per-document view state, and the collaboration
//!     session display: phase, roster, notice, join panel);
//!   - the host caches derived from it (`DocumentCaches`). Each document's
//!     revision restarts at 0, so a cache keyed by revision would otherwise
//!     serve one document's scene or LayerPanel rows to another;
//!   - its blur-up thumbnail registry, which is process-global in jian;
//!   - its document epoch, so async work captured for one document is still
//!     refused after the user switches away and back.
//!
//! App-level state (theme, locale, agent connections, chat, clipboard, window
//! chrome, collaboration availability / transports / relay region) never moves.
//!
//! A collaboration session stays attached to ONE document while the user
//! switches tabs, so switching is not a replacement and `swap_document` does
//! not ask the collaboration gate. What a parked document may be used for
//! depends on that document's own session, not on the one on screen: see the
//! four entry points ([`WidgetHostNative::swap_document`], `with_parked_document`,
//! `with_parked_collab_session`, `read_parked_document`).

use super::*;
use jian_ops_schema::image_thumbs::{capture_snapshot, restore_snapshot, ImageThumbSnapshot};
use std::sync::atomic::{AtomicU64, Ordering};

/// Source of document epochs. Process-wide so an epoch is never reused by any
/// document, which is what lets the Phase-0 stale-apply checks tell documents
/// apart: a per-host `+ 1` would hand two open documents the same value.
static NEXT_DOCUMENT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Allocate a document epoch that no document has used or will use again.
pub fn next_document_epoch() -> u64 {
    NEXT_DOCUMENT_EPOCH.fetch_add(1, Ordering::Relaxed)
}

/// Host caches derived from the document on screen, parked with it.
struct DocumentCaches {
    layout_scene: op_editor_ui::layout_scene::LayoutScene,
    scene_cache: op_pen_loader::SceneBuildCache,
    layout_transition: Option<op_editor_ui::widgets::CanvasLayoutTransition>,
    editor_state_dirty: bool,
    layout_scene_font_generation: u64,
    layer_panel_owner: u64,
    slide_thumbs: slides_panel_thumbs::SlideThumbCache,
    next_node_id: u64,
    collab_id_allocator: Option<op_editor_core::DocumentIdAllocator>,
}

impl DocumentCaches {
    /// Empty caches for a document that has never been on screen. The scene is
    /// rebuilt on first paint (`editor_state_dirty`), the LayerPanel gets a
    /// fresh owner, and node ids resume above everything the document holds.
    fn fresh(doc: &jian_ops_schema::PenDocument) -> Self {
        let next_node_id = op_editor_core::next_sequential_counter(doc)
            .unwrap_or(100)
            .max(100);
        Self {
            layout_scene: Default::default(),
            scene_cache: op_pen_loader::SceneBuildCache::new(),
            layout_transition: None,
            editor_state_dirty: true,
            layout_scene_font_generation: 0,
            layer_panel_owner: op_editor_ui::widgets::LayerPanel::next_layer_panel_owner(),
            slide_thumbs: Default::default(),
            next_node_id,
            collab_id_allocator: None,
        }
    }
}

/// A document that is open but not on screen.
pub struct ParkedDocument {
    scope: op_editor_core::DocumentScope,
    caches: DocumentCaches,
    thumbs: ImageThumbSnapshot,
    epoch: u64,
}

impl ParkedDocument {
    /// Wrap a freshly built state as a document that has not been on screen.
    ///
    /// `thumbs` is the thumbnail registry the state's loader produced (see
    /// [`capture_loaded_thumbnails`]); pass the default for a state with none.
    pub fn from_state(state: op_editor_core::EditorState, thumbs: ImageThumbSnapshot) -> Self {
        let caches = DocumentCaches::fresh(&state.doc);
        Self {
            scope: op_editor_core::DocumentScope::from_state(state),
            caches,
            thumbs,
            epoch: next_document_epoch(),
        }
    }

    /// Epoch of this document (see [`WidgetHostNative::document_epoch`]).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The parked document's editor data.
    pub fn scope(&self) -> &op_editor_core::DocumentScope {
        &self.scope
    }

    /// Mutable access, for a save acknowledgement that reaches a parked tab.
    pub fn scope_mut(&mut self) -> &mut op_editor_core::DocumentScope {
        &mut self.scope
    }

    /// Whether this document has edits since its last saved baseline.
    pub fn is_dirty(&self) -> bool {
        self.scope.is_dirty()
    }

    /// Whether a collaboration session is bound to this document (its phase
    /// is anything but `Idle` / `Discovering`).
    pub fn collab_session_is_bound(&self) -> bool {
        self.scope.collab().binds_session()
    }

    /// Whether this is another person's shared document, held as a guest.
    /// See [`op_editor_core::CollabUiState::holds_a_guests_shared_document`].
    pub fn holds_a_guests_shared_document(&self) -> bool {
        self.scope.collab().holds_a_guests_shared_document()
    }
}

/// Run a document loader and return its result together with the thumbnail
/// registry the loaded document produced.
///
/// Loading activates the document's thumbnails into the process-global
/// registry, replacing whatever the document on screen was using. This brackets
/// the load: the live registry is saved, the registry is emptied so the loaded
/// document's own table (or none) is what ends up captured, and the live
/// registry is put back before returning. The returned snapshot belongs to the
/// loaded document and is handed to [`ParkedDocument::from_state`].
pub fn capture_loaded_thumbnails<T>(load: impl FnOnce() -> T) -> (T, ImageThumbSnapshot) {
    let live = capture_snapshot();
    restore_snapshot(ImageThumbSnapshot::default());
    let loaded = load();
    let loaded_thumbs = capture_snapshot();
    restore_snapshot(live);
    (loaded, loaded_thumbs)
}

impl WidgetHostNative {
    /// Exchange the document on screen with `parked`: afterwards `parked`
    /// holds the document that was showing and the host shows the one that was
    /// parked.
    ///
    /// A collaboration session does not forbid this: it stays attached to the
    /// document it belongs to and travels with it, so showing another tab is
    /// not a replacement. The caller decides whether a switch is safe right now
    /// (for example, not in the middle of a local edit capture).
    ///
    /// Pending input is committed first so a half-typed edit lands in the
    /// document it was typed in, and every transient gesture or runtime tied to
    /// the outgoing document is cancelled so none of it can act on the
    /// incoming one.
    pub fn swap_document(&mut self, parked: &mut ParkedDocument) -> bool {
        self.commit_pending_input_for_switch();
        self.drop_preview_runtime();
        self.cancel_document_gestures();

        self.exchange_parked_state(parked, true);

        // The transcript cache is keyed by chat session, not by document, so it
        // does not travel; rotate it so the first frame cannot pair the
        // previous document's cached geometry with this one's messages.
        self.force_rotate_chat_owner();
        // The scene cache short-circuits when its inputs match, so this is
        // cheap for a document that was parked with a current scene, and it
        // also covers a font-registry change while the document was parked.
        self.editor_state_dirty = true;
        self.drop_pan_cache();
        true
    }

    /// Commit every half-finished input (a property draft, a layer rename, a
    /// canvas text edit) into the document it was typed in. Idempotent. A host
    /// that shares the document with other people runs this inside its own
    /// edit capture BEFORE a switch, so the commit reaches them.
    pub fn commit_pending_input_for_switch(&mut self) {
        self.commit_pending_input_pub();
        self.editor_state.rename_commit();
        self.editor_state.text_edit_commit();
    }

    /// Exchange everything that makes a document what it is: its editor data,
    /// thumbnails, epoch and derived caches. No side effect is visible to the
    /// user, which is what lets [`Self::with_parked_document`] reuse it.
    /// `user_switch` also carries the app-level chat preferences onto the
    /// arriving document (see `EditorState::switch_document_scope`).
    fn exchange_parked_state(&mut self, parked: &mut ParkedDocument, user_switch: bool) {
        if user_switch {
            self.editor_state.switch_document_scope(&mut parked.scope);
        } else {
            self.editor_state.exchange_document_scope(&mut parked.scope);
        }

        let live_thumbs = capture_snapshot();
        restore_snapshot(std::mem::replace(&mut parked.thumbs, live_thumbs));

        std::mem::swap(&mut self.document_epoch, &mut parked.epoch);
        let caches = &mut parked.caches;
        std::mem::swap(&mut self.layout_scene, &mut caches.layout_scene);
        std::mem::swap(&mut self.scene_cache, &mut caches.scene_cache);
        std::mem::swap(&mut self.layout_transition, &mut caches.layout_transition);
        std::mem::swap(&mut self.editor_state_dirty, &mut caches.editor_state_dirty);
        std::mem::swap(
            &mut self.layout_scene_font_generation,
            &mut caches.layout_scene_font_generation,
        );
        std::mem::swap(&mut self.layer_panel_owner, &mut caches.layer_panel_owner);
        std::mem::swap(&mut self.slide_thumbs, &mut caches.slide_thumbs);
        std::mem::swap(&mut self.next_node_id, &mut caches.next_node_id);
        std::mem::swap(
            &mut self.collab_id_allocator,
            &mut caches.collab_id_allocator,
        );
    }

    /// Run `f` with the parked document standing in for the one on screen, then
    /// put everything back. This is how a run bound to a document that is not
    /// showing keeps streaming into it: its pump calls `editor_state_mut()` as
    /// usual and reaches the parked document.
    ///
    /// Unlike [`Self::swap_document`] this is invisible to the user: pending
    /// input is not committed, gestures and the preview runtime are left
    /// alone, and no cache is dropped or rotated, so the on-screen document is
    /// restored exactly as it was. Both exchanges are pointer swaps (the
    /// thumbnail registry clones a map of `Arc`s).
    ///
    /// Returns `None` without running `f` (and without a toast) when a
    /// collaboration session is bound to the PARKED document: general writers
    /// (an agent run, an MCP write, an import) must not edit a document other
    /// people are editing. The refusal depends on the target only; whatever is
    /// on screen is irrelevant, so a plain parked document stays reachable
    /// while another tab collaborates. The session's own pump uses
    /// [`Self::with_parked_collab_session`], and a read-only look (a recovery
    /// snapshot) uses [`Self::read_parked_document`]. `f` must not call
    /// `swap_document` or this method.
    pub fn with_parked_document<R>(
        &mut self,
        parked: &mut ParkedDocument,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        // A refusal here is routine (it repeats every frame a parked run is
        // waiting), so it must not raise a toast.
        if parked.collab_session_is_bound() {
            return None;
        }
        Some(self.run_lens(parked, f))
    }

    /// The lens for the collaboration runtime's own pump: poll, drain events,
    /// leave. The ONLY entry that lets code mutate a parked document that has a
    /// session bound; it returns `None` for any other document so it cannot be
    /// used as a way around [`Self::with_parked_document`].
    ///
    /// On top of the plain lens it keeps the pan bitmap cache of the document
    /// on screen: applying a remote change marks the (parked) document dirty,
    /// and that path drops the host's pan cache, which belongs to whatever is
    /// showing. Nothing the user sees may change because another tab received
    /// an update.
    pub fn with_parked_collab_session<R>(
        &mut self,
        parked: &mut ParkedDocument,
        f: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if !parked.collab_session_is_bound() {
            return None;
        }
        let on_screen_pan_cache = (self.pan_cache.take(), self.pan_cache_restore.take());
        let result = self.run_lens(parked, f);
        (self.pan_cache, self.pan_cache_restore) = on_screen_pan_cache;
        Some(result)
    }

    /// Look at a parked document without being able to change it: `f` gets a
    /// shared reference to the host with the document standing in. Allowed for
    /// every document, collaborated or not, because nothing can be written
    /// through it. A crash-recovery snapshot reads through this.
    pub fn read_parked_document<R>(
        &mut self,
        parked: &mut ParkedDocument,
        f: impl FnOnce(&Self) -> R,
    ) -> R {
        self.exchange_parked_state(parked, false);
        let result = f(self);
        self.exchange_parked_state(parked, false);
        result
    }

    fn run_lens<R>(&mut self, parked: &mut ParkedDocument, f: impl FnOnce(&mut Self) -> R) -> R {
        self.exchange_parked_state(parked, false);
        let result = f(self);
        self.exchange_parked_state(parked, false);
        result
    }

    /// Whether a pointer gesture or a canvas text edit is in progress: a drag
    /// of the canvas, a node, a handle or a slider, or a caret in a text node.
    /// The document is changing under the user's hand, so a snapshot taken now
    /// would capture a half-finished edit; background writers wait it out.
    pub fn pointer_gesture_in_progress(&self) -> bool {
        self.drag.is_some()
            || self.node_drag.is_some()
            || self.handle_drag.is_some()
            || self.rotate_drag.is_some()
            || self.create_drag.is_some()
            || self.marquee_drag.is_some()
            || self.layer_drag.is_some()
            || self.path_anchor_drag.is_some()
            || self.arc_handle_drag.is_some()
            || self.image_adjustment_drag.is_some()
            || self.image_crop_drag.is_some()
            || self.effect_radius_drag.is_some()
            || self.text_edit_selection_drag.is_some()
            || self.editor_state.ui.text_editing.is_some()
    }

    /// Cancel every in-flight pointer gesture and per-node drag. They hold
    /// node ids and screen anchors of the document that is leaving.
    fn cancel_document_gestures(&mut self) {
        self.drag = None;
        self.node_drag = None;
        self.handle_drag = None;
        self.rotate_drag = None;
        self.create_drag = None;
        self.marquee_drag = None;
        self.layer_drag = None;
        self.path_anchor_drag = None;
        self.arc_handle_drag = None;
        self.canvas_drop_index = None;
        self.option_drag_source_ids.clear();
        self.image_adjustment_drag = None;
        self.image_crop_drag = None;
        self.effect_radius_drag = None;
        self.code_selection_drag = None;
        self.text_edit_selection_drag = None;
        self.last_hover_probe = None;
        self.drop_pan_cache();
    }
}

#[cfg(test)]
#[path = "document_swap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "document_swap_lens_tests.rs"]
mod lens_tests;

#[cfg(test)]
#[path = "document_swap_collab_tests.rs"]
mod collab_tests;
