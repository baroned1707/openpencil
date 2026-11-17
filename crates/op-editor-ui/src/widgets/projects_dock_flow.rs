//! Shared press / hover / scroll flow for the Projects dock.
//!
//! Both hosts would hit-test the same layout and run the same transitions,
//! so all of it lives here and a host keeps only its platform tail (a
//! repaint). The dock's outer rect comes from `host_canvas_geometry::
//! projects_dock_rect` — the same rect the canvas and the property panel
//! subtract — and the list inside it is `projects_panel_flow`'s.

use op_editor_core::{EditorState, ProjectRequest};

use crate::widgets::host_canvas_geometry::{projects_dock_rect, projects_dock_width};
use crate::widgets::projects_dock::ProjectsDock;
use crate::widgets::projects_dock_layout::{DockHit, ProjectsDockLayout};
use crate::widgets::projects_panel_flow::{self, ProjectsPress};
use crate::Point2D;

/// What a press on the dock amounted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockPress {
    /// The point is not on the dock; the host falls through.
    Missed,
    /// The dock owns the press (a chevron, a row, or bare dock surface);
    /// nothing below may see it.
    Claimed,
}

/// The dock's layout for a `viewport_w` x `viewport_h` window, or `None`
/// while the dock is not shown.
pub fn layout(state: &EditorState, viewport_w: f32, viewport_h: f32) -> Option<ProjectsDockLayout> {
    if projects_dock_width(state) <= 0.0 {
        return None;
    }
    let panel = &state.editor_ui.projects_panel;
    Some(ProjectsDockLayout::new(
        &panel.projects,
        projects_dock_rect(state, viewport_w, viewport_h),
        panel.is_open(),
        panel.scroll.offset,
    ))
}

/// The dock widget for the current state, ready to paint. `text` is the
/// list's strings (`projects_panel_flow::text`) and `title` the header's.
pub fn widget<'a>(
    state: &'a EditorState,
    text: &'a crate::widgets::ProjectsPanelText<'a>,
    title: &'a str,
    now_ms: u64,
) -> ProjectsDock<'a> {
    ProjectsDock {
        list: projects_panel_flow::widget(state, text, now_ms),
        title,
        toggle_hover: state.editor_ui.projects_panel.toggle_hover,
        open_project_hover: state.editor_ui.projects_panel.open_project_hover,
        has_projects: !state.editor_ui.projects_panel.projects.is_empty(),
    }
}

/// The header title, translated.
pub fn title(state: &EditorState) -> &'static str {
    crate::widgets::editor_state_ext::translate(&state.editor_ui, "leftPanel.tab.projects")
}

/// Fold the dock into its strip, or open it again. Drops the hover that
/// belonged to the layout being replaced, and a rename whose row is now hidden.
pub fn toggle(state: &mut EditorState) {
    let panel = &mut state.editor_ui.projects_panel;
    panel.toggle_open();
    panel.drop_hidden_rename();
    panel.hover = None;
    panel.toggle_hover = false;
    panel.open_project_hover = false;
}

/// Route a press at `point` at `now_ms`: the chevron toggles, a row records the
/// request it stands for (or starts a rename), anything else on the dock is
/// swallowed.
pub fn press(
    state: &mut EditorState,
    layout: &ProjectsDockLayout,
    point: Point2D,
    now_ms: u64,
) -> DockPress {
    // An open rename settles first, whatever the press lands on.
    let list = layout.list.as_ref();
    if projects_panel_flow::rename_press(state, list, point)
        == projects_panel_flow::RenamePress::InField
    {
        return DockPress::Claimed;
    }
    // A "Move to Trash?" question ends with any press off its own row; a press
    // on the list is settled by the list itself (it must see where it landed).
    if !matches!(layout.hit(point), Some(DockHit::Body(_))) {
        projects_panel_flow::settle_trash_confirm(state, list, point);
    }
    match layout.hit(point) {
        None => DockPress::Missed,
        Some(DockHit::Toggle) => {
            toggle(state);
            DockPress::Claimed
        }
        Some(DockHit::OpenProject) => {
            state.editor_ui.projects_panel.pending_request =
                Some(ProjectRequest::OpenProjectPicker);
            DockPress::Claimed
        }
        Some(DockHit::Header) => DockPress::Claimed,
        Some(DockHit::Body(_)) => {
            if let Some(list) = &layout.list {
                // The list's verdict is irrelevant: the point is on the dock
                // either way, so the press is ours.
                let _: ProjectsPress = projects_panel_flow::press(state, list, point, now_ms);
            }
            DockPress::Claimed
        }
    }
}

/// Track the cursor. Returns whether any hover changed (the repaint signal).
pub fn cursor_move(state: &mut EditorState, layout: &ProjectsDockLayout, point: Point2D) -> bool {
    let over_toggle = matches!(layout.hit(point), Some(DockHit::Toggle));
    let over_open = matches!(layout.hit(point), Some(DockHit::OpenProject));
    let panel = &mut state.editor_ui.projects_panel;
    let toggle_changed = panel.toggle_hover != over_toggle || panel.open_project_hover != over_open;
    panel.toggle_hover = over_toggle;
    panel.open_project_hover = over_open;
    let list_changed = match &layout.list {
        Some(list) => projects_panel_flow::cursor_move(state, list, point),
        None => projects_panel_flow::clear_hover(state),
    };
    toggle_changed || list_changed
}

/// Drop every hover (the cursor left the dock). Returns whether one was set.
pub fn clear_hover(state: &mut EditorState) -> bool {
    let list_cleared = projects_panel_flow::clear_hover(state);
    let panel = &mut state.editor_ui.projects_panel;
    let toggle_cleared = std::mem::take(&mut panel.toggle_hover);
    let open_cleared = std::mem::take(&mut panel.open_project_hover);
    list_cleared || toggle_cleared || open_cleared
}

/// Scroll the list. `None` when the pointer is not on the dock, so the
/// caller keeps looking; over the header or the collapsed strip the wheel is
/// swallowed (`Some(false)`) rather than zooming the canvas through the dock.
pub fn scroll(
    state: &mut EditorState,
    layout: Option<&ProjectsDockLayout>,
    point: Point2D,
    delta_y: f32,
) -> Option<bool> {
    let layout = layout?;
    if !layout.contains_point(point) {
        return None;
    }
    Some(projects_panel_flow::scroll(state, layout.list.as_ref(), point, delta_y).unwrap_or(false))
}

#[cfg(test)]
#[path = "projects_dock_flow_tests.rs"]
mod projects_dock_flow_tests;
