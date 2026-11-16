//! Where a Scene Template Center request that REPLACES the document lands.
//!
//! "Generate from a topic" and "open a template onto a blank page" both throw
//! the document on screen away and put a new one in its place. With several
//! documents open as tabs that must never take an open document with it: the
//! replacement is installed only into a pristine untitled tab
//! (`active_document_is_pristine_untitled`); for any other document a NEW
//! untitled tab is opened first and the replacement goes into that tab, so the
//! original stays in its own tab untouched. If a tab cannot be opened right
//! now the request is refused (the usual "cannot switch" notice) and nothing
//! changes.
//!
//! The order is fixed by the call sequence: prepare (no replacement) → make
//! the target tab safe → install. The install functions are never reached
//! before the tab step has returned `true`.

use crate::scene_template_generate::{
    install_generate_document, prepare_pending_scene_template_generate,
};
use crate::scene_template_open::{
    install_template_replacement, prepare_pending_scene_template, TemplateRequest,
};
use crate::DesktopApp;

impl DesktopApp {
    /// Make sure the document on screen may be replaced: it already is a
    /// pristine untitled one, or a new untitled tab has just been opened.
    /// `false` (after telling the user) when a tab cannot be opened.
    pub(crate) fn ensure_replaceable_document(&mut self) -> bool {
        self.active_document_is_pristine_untitled() || self.open_untitled_tab()
    }

    /// Drain a template the Scene Template Center chose. Returns whether the
    /// frame must repaint.
    pub(crate) fn drain_scene_template_open(&mut self, now_ms: u64) -> bool {
        match prepare_pending_scene_template(
            &mut self.host,
            &mut self.current_path,
            self.window.as_ref(),
            now_ms,
        ) {
            TemplateRequest::Handled(changed) => changed,
            TemplateRequest::Replace(state) => {
                if !self.ensure_replaceable_document() {
                    // Refused: the notice is already up; the request is spent.
                    return true;
                }
                install_template_replacement(
                    &mut self.host,
                    &mut self.current_path,
                    self.window.as_ref(),
                    *state,
                )
            }
        }
    }

    /// Drain a generate request from the Scene Template Center. Returns true
    /// when a document was prepared and a chat turn queued for it.
    pub(crate) fn drain_scene_template_generate(&mut self) -> bool {
        let Some(request) = prepare_pending_scene_template_generate(&mut self.host) else {
            return false;
        };
        if !self.ensure_replaceable_document() {
            return false;
        }
        // A new tab starts without the style pin (it is document-scoped);
        // carry the one the request was raised with.
        self.host.editor_state_mut().editor_ui.pinned_style_guide =
            request.pinned_style_guide.clone();
        install_generate_document(
            &mut self.host,
            &mut self.current_path,
            self.window.as_ref(),
            &request,
        )
    }
}

#[cfg(test)]
#[path = "scene_template_tab_host_tests.rs"]
mod tests;
