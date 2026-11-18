//! File ▸ Copy image credits: put the document's stock-image credit lines on
//! the clipboard and say how many were copied.

use op_editor_core::editor_toast::EditorToastLevel;
use op_host_native::widget_host::WidgetHostNative;

/// Toast keys: singular, plural (`{{count}}`) and the empty document.
const COPIED_ONE_KEY: &str = "imageCredits.copiedOne";
const COPIED_KEY: &str = "imageCredits.copied";
const NONE_KEY: &str = "imageCredits.none";

/// Copy the credits of every page of the document on screen. A document
/// without credits copies nothing (the clipboard is left alone) and says so.
pub(crate) fn copy_image_credits(host: &mut WidgetHostNative) {
    let lines = op_image_enrich::collect_image_credits(&host.editor_state().doc);
    let now_ms = host.now_ms();
    let state = host.editor_state_mut();
    if lines.is_empty() {
        state
            .editor_ui
            .show_toast(NONE_KEY, Vec::new(), EditorToastLevel::Info, now_ms);
    } else {
        state
            .chat
            .queue_copy_text(op_image_enrich::format_credits_text(&lines));
        let (key, args) = if lines.len() == 1 {
            (COPIED_ONE_KEY, Vec::new())
        } else {
            (
                COPIED_KEY,
                vec![("count".to_string(), lines.len().to_string())],
            )
        };
        state
            .editor_ui
            .show_toast(key, args, EditorToastLevel::Info, now_ms);
    }
    host.mark_editor_state_dirty();
}

#[cfg(test)]
#[path = "image_credits_host_tests.rs"]
mod tests;
