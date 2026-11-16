//! Press / hover / scroll flow for the Projects dock's project list.
//!
//! The dock (`projects_dock_flow`) places the list and routes presses that
//! land on it here, so the list transitions live once. A press ACTS on
//! press, like the Layers tree rows: the list has no drag gesture that would
//! need a release to disambiguate it.
//!
//! The list never mutates the project rows — the host owns them. A click
//! only records a [`ProjectRequest`] on `projects_panel.pending_request`,
//! which the host drains.

use op_editor_core::{EditorState, ProjectRequest, ProjectRowTarget};

use crate::widgets::projects_panel::{ProjectsPanel, ProjectsPanelText};
use crate::widgets::projects_panel_layout::ProjectsPanelLayout;
use crate::Point2D;

/// What a press on the list amounted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectsPress {
    /// The point is not on the list; the host falls through.
    Missed,
    /// The list owns the press. It landed on a row when `Some`, and on bare
    /// body when `None`; either way nothing below may see it.
    Claimed(Option<ProjectRowTarget>),
}

/// The list's translated strings, resolved once per frame.
pub fn text(state: &EditorState) -> ProjectsPanelText<'static> {
    let ui = &state.editor_ui;
    let translate = crate::widgets::editor_state_ext::translate;
    ProjectsPanelText {
        empty: translate(ui, "projects.empty"),
        new_design: translate(ui, "projects.newDesign"),
        needs_review: translate(ui, "projects.needsReview"),
        running: translate(ui, "projects.running"),
    }
}

/// The widget for the current state, ready to paint.
pub fn widget<'a>(state: &'a EditorState, text: &'a ProjectsPanelText<'a>) -> ProjectsPanel<'a> {
    ProjectsPanel {
        projects: &state.editor_ui.projects_panel.projects,
        hover: state.editor_ui.projects_panel.hover,
        text,
    }
}

/// The request a press on `target` raises, if the row it names still
/// exists. Indices come from the layout built from the same rows, so a
/// miss only happens when the host swapped the rows mid-event.
fn request_for(state: &EditorState, target: ProjectRowTarget) -> Option<ProjectRequest> {
    let projects = &state.editor_ui.projects_panel.projects;
    match target {
        ProjectRowTarget::Header(project) => Some(ProjectRequest::ToggleProject(
            projects.get(project)?.root.clone(),
        )),
        ProjectRowTarget::Plus(project) | ProjectRowTarget::NewDesignRow(project) => Some(
            ProjectRequest::NewDesignIn(projects.get(project)?.root.clone()),
        ),
        ProjectRowTarget::File(project, file) => {
            let file = projects.get(project)?.files.get(file)?;
            Some(match file.open_id {
                Some(id) => ProjectRequest::SwitchTo(id),
                None => ProjectRequest::OpenFile(file.path.clone()),
            })
        }
    }
}

/// Route a press at `point`: record the request the row stands for.
pub fn press(
    state: &mut EditorState,
    layout: &ProjectsPanelLayout,
    point: Point2D,
) -> ProjectsPress {
    if !layout.contains_point(point) {
        return ProjectsPress::Missed;
    }
    let mut hit = layout.hit(point);
    let ui = &mut state.editor_ui.projects_panel;
    // The `+` only shows while its header is hovered. A press that never
    // saw a move first (a synthesized click) cannot have aimed at a button
    // the user could not see, so it counts as a press on the header.
    if let Some(ProjectRowTarget::Plus(project)) = hit {
        if !matches!(
            ui.hover,
            Some(ProjectRowTarget::Header(p) | ProjectRowTarget::Plus(p)) if p == project
        ) {
            hit = Some(ProjectRowTarget::Header(project));
        }
    }
    ui.hover = hit;
    if let Some(target) = hit {
        let request = request_for(state, target);
        state.editor_ui.projects_panel.pending_request = request;
    }
    ProjectsPress::Claimed(hit)
}

/// Track the cursor. Returns whether the hover changed (the repaint signal).
pub fn cursor_move(state: &mut EditorState, layout: &ProjectsPanelLayout, point: Point2D) -> bool {
    let hover = layout.hit(point);
    let panel = &mut state.editor_ui.projects_panel;
    let changed = panel.hover != hover;
    panel.hover = hover;
    changed
}

/// Drop the hover (the cursor left the list). Returns whether one was set.
pub fn clear_hover(state: &mut EditorState) -> bool {
    state.editor_ui.projects_panel.hover.take().is_some()
}

/// Scroll the list. `None` when the pointer is not over the list, so
/// the caller keeps looking. `delta_y` follows the wheel convention the
/// slides list uses: positive scrolls toward the top.
pub fn scroll(
    state: &mut EditorState,
    layout: Option<&ProjectsPanelLayout>,
    point: Point2D,
    delta_y: f32,
) -> Option<bool> {
    let layout = layout?;
    if !layout.contains_point(point) {
        return None;
    }
    let scroll = &mut state.editor_ui.projects_panel.scroll;
    let before = scroll.offset;
    // Start from the offset the rows are actually drawn at, so a stale
    // over-large stored offset cannot swallow the first wheel notch.
    scroll.offset = layout.offset;
    crate::util::scroll_by_max(scroll, -delta_y, layout.max_scroll());
    Some(scroll.offset != before)
}

#[cfg(test)]
#[path = "projects_panel_flow_tests.rs"]
pub(crate) mod projects_panel_flow_tests;
