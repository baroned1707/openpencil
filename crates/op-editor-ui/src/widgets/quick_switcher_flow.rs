//! Shared press / hover / wheel flow for the quick switcher popup.
//!
//! The switcher is modal while open: a press outside the card closes it and is
//! swallowed, a press on a row chooses it, everything else the card owns. Both
//! hosts would run the same transitions, so they live here and a host keeps only
//! its platform tail (a repaint). State changes go through
//! `op_editor_core::quick_switcher_keyboard`.

use op_editor_core::{quick_switcher_keyboard as switcher, EditorState};

use crate::widgets::quick_switcher::{
    paint as paint_card, text_for, QuickSwitcherHit, QuickSwitcherLayout,
};
use crate::widgets::PaintCx;
use crate::Point2D;

/// The card's layout, or `None` while the switcher is closed.
pub fn layout(state: &EditorState, viewport_w: f32) -> Option<QuickSwitcherLayout> {
    let switcher = &state.editor_ui.projects_panel.switcher;
    switcher.open.then(|| {
        QuickSwitcherLayout::new(
            switcher::rows(state).len(),
            switcher.first_visible,
            viewport_w,
        )
    })
}

/// Paint the open switcher; a no-op while it is closed.
pub fn paint(cx: &mut PaintCx<'_>, state: &EditorState, viewport_w: f32, now_ms: u64) {
    let Some(layout) = layout(state, viewport_w) else {
        return;
    };
    let rows = switcher::rows(state);
    let theme = crate::widgets::editor_state_ext::theme_for(&state.editor_ui);
    let text = text_for(state.editor_ui.effective_locale());
    paint_card(
        cx,
        &theme,
        &layout,
        &state.editor_ui.projects_panel.switcher,
        &rows,
        &text,
        now_ms,
    );
}

/// Route a press at `point`: outside the card closes the switcher, a row
/// chooses it. `false` only while the switcher is closed; an open switcher
/// claims every press.
pub fn press(state: &mut EditorState, viewport_w: f32, point: Point2D) -> bool {
    let Some(layout) = layout(state, viewport_w) else {
        return false;
    };
    match layout.hit(point) {
        QuickSwitcherHit::Outside => {
            switcher::close(state);
        }
        QuickSwitcherHit::Row(index) => {
            switcher::choose(state, index);
        }
        QuickSwitcherHit::Inside => {}
    }
    true
}

/// Track the pointer: a hovered row becomes the highlighted one. `None` while
/// the switcher is closed (the cursor goes on to the tiers below); an open
/// switcher owns the cursor, `Some(changed)` says whether to repaint.
pub fn cursor_move(state: &mut EditorState, viewport_w: f32, point: Point2D) -> Option<bool> {
    let layout = layout(state, viewport_w)?;
    Some(match layout.hit(point) {
        QuickSwitcherHit::Row(index) => switcher::select_row(state, index),
        _ => false,
    })
}

/// The wheel: scrolls the list by whole rows. `None` while the switcher is
/// closed; an open switcher swallows every wheel event so the canvas beneath
/// never zooms.
pub fn scroll(state: &mut EditorState, delta_y: f32) -> Option<bool> {
    if !switcher::is_active(state) {
        return None;
    }
    // Same sign convention as the dock's list: positive scrolls toward the top.
    let rows = if delta_y > 0.0 {
        -1
    } else if delta_y < 0.0 {
        1
    } else {
        0
    };
    Some(switcher::scroll(state, rows))
}

#[cfg(test)]
#[path = "quick_switcher_flow_tests.rs"]
mod tests;
