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

use op_editor_core::project_rename::{is_renamable, DOUBLE_CLICK_MS};
use op_editor_core::{
    DocumentId, DocumentTabRequest, EditorState, ProjectFileKind, ProjectRequest, ProjectRowTarget,
};

use crate::widgets::projects_panel::{rename_field_rect, ProjectsPanel, ProjectsPanelText};
use crate::widgets::projects_panel_layout::{ProjectsPanelLayout, RowKind};
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
        open_project: translate(ui, "project.open"),
        open_hint: translate(ui, "project.openFolderHint"),
        close: translate(ui, "project.close"),
        refresh: translate(ui, "project.refresh"),
        scanning: translate(ui, "project.scanning"),
        slow: translate(ui, "project.slow"),
        unreadable: translate(ui, "project.unreadable"),
        truncated: translate(ui, "project.truncated"),
        no_designs: translate(ui, "project.noDesigns"),
        fig_convert: translate(ui, "project.figConvert"),
        fig_converted: translate(ui, "project.figConverted"),
        fig_converting: translate(ui, "project.figConverting"),
        fig_failed: translate(ui, "project.figFailed"),
        fig_convert_all: translate(ui, "project.figConvertAll"),
        fig_convert_tip: translate(ui, "project.figConvertTip"),
        fig_converted_tip: translate(ui, "project.figConvertedTip"),
        fig_failed_tip: translate(ui, "project.figFailedTip"),
        fig_convert_all_tip: translate(ui, "project.figConvertAllTip"),
        rename: translate(ui, "projects.rename"),
        rename_placeholder: translate(ui, "projects.renamePlaceholder"),
        rename_invalid: translate(ui, "projects.renameInvalid"),
        rename_exists: translate(ui, "projects.renameExists"),
        trash: translate(ui, "projects.trash"),
        trash_confirm: translate(ui, "projects.trashConfirm"),
        cancel: translate(ui, "common.cancel"),
        new_here: translate(ui, "projects.newDesignHere"),
        not_on_disk: translate(ui, "projects.notOnDisk"),
        close_file: translate(ui, "common.close"),
        open_no_project: translate(ui, "projects.openNoProject"),
    }
}

/// The widget for the current state, ready to paint. `now_ms` drives the
/// caret blink of an open rename.
pub fn widget<'a>(
    state: &'a EditorState,
    text: &'a ProjectsPanelText<'a>,
    now_ms: u64,
) -> ProjectsPanel<'a> {
    ProjectsPanel {
        projects: &state.editor_ui.projects_panel.projects,
        tabs: &state.editor_ui.document_tabs,
        hover: state.editor_ui.projects_panel.hover,
        rename: state.editor_ui.projects_panel.rename.as_ref(),
        trash_confirm: state.editor_ui.projects_panel.trash_confirm.as_deref(),
        now_ms,
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
        ProjectRowTarget::Close(project) => Some(ProjectRequest::CloseProject(
            projects.get(project)?.root.clone(),
        )),
        ProjectRowTarget::Refresh(project) => Some(ProjectRequest::RefreshProject(
            projects.get(project)?.root.clone(),
        )),
        ProjectRowTarget::Folder(project, file) => {
            let view = projects.get(project)?;
            Some(ProjectRequest::ToggleFolder(
                view.root.clone(),
                view.files.get(file)?.folder.clone(),
            ))
        }
        ProjectRowTarget::OpenProject => Some(ProjectRequest::OpenProjectPicker),
        // Starting a rename or a "Move to Trash?" question is the list's own
        // state, not a host request.
        ProjectRowTarget::RenameFile(..) | ProjectRowTarget::TrashFile(..) => None,
        // Closing and switching to an open document are document requests
        // (`document_request_for`), not project requests.
        ProjectRowTarget::CloseFile(..)
        | ProjectRowTarget::OpenDocument(_)
        | ProjectRowTarget::CloseOpenDocument(_) => None,
        ProjectRowTarget::NewHere(project, folder_file) => {
            let view = projects.get(project)?;
            let folder = match folder_file {
                Some(index) => view.files.get(index)?.folder.clone(),
                None => String::new(),
            };
            Some(ProjectRequest::NewDesignHere(view.root.clone(), folder))
        }
        ProjectRowTarget::ConvertAll(project, first_file) => {
            let view = projects.get(project)?;
            let folder = match first_file {
                Some(index) => Some(view.files.get(index)?.folder.clone()),
                None => None,
            };
            Some(ProjectRequest::ConvertAllFigs(view.root.clone(), folder))
        }
        ProjectRowTarget::File(project, file) => {
            let file = projects.get(project)?.files.get(file)?;
            // A `.fig` is never opened as a document from here: opening one
            // would run the whole-file Figma import into a tab. The host turns
            // the click into a conversion, or opens the `.op` beside it.
            if matches!(file.kind, ProjectFileKind::Figma(_)) {
                return Some(ProjectRequest::ConvertFig(file.path.clone()));
            }
            Some(match file.open_id {
                Some(id) => ProjectRequest::SwitchTo(id),
                None => ProjectRequest::OpenFile(file.path.clone()),
            })
        }
    }
}

/// The open document a row or button stands for: an open file's row and its
/// buttons, or a row of the "Open, not in a project" section.
fn open_document_at(state: &EditorState, target: ProjectRowTarget) -> Option<DocumentId> {
    let panel = &state.editor_ui.projects_panel;
    match target {
        ProjectRowTarget::File(project, file)
        | ProjectRowTarget::RenameFile(project, file)
        | ProjectRowTarget::TrashFile(project, file)
        | ProjectRowTarget::CloseFile(project, file) => {
            panel.projects.get(project)?.files.get(file)?.open_id
        }
        ProjectRowTarget::OpenDocument(tab) | ProjectRowTarget::CloseOpenDocument(tab) => {
            state.editor_ui.document_tabs.get(tab).map(|tab| tab.id)
        }
        _ => None,
    }
}

/// The document request a press on `target` raises: the close (x) of an open
/// file or of an open-document row closes it (the host runs the same close as
/// Cmd+W, so unsaved work asks first), and a press on an open-document row
/// shows it.
fn document_request_for(
    state: &EditorState,
    target: ProjectRowTarget,
) -> Option<DocumentTabRequest> {
    let id = open_document_at(state, target)?;
    match target {
        ProjectRowTarget::CloseFile(..) | ProjectRowTarget::CloseOpenDocument(_) => {
            Some(DocumentTabRequest::Close(id))
        }
        ProjectRowTarget::OpenDocument(_) => Some(DocumentTabRequest::Switch(id)),
        _ => None,
    }
}

/// A middle press at `point`: on an open file's row or an open-document row it
/// closes that document, like a browser tab; anywhere else on the list it does
/// nothing. The list owns the press either way (so it never starts a canvas
/// pan under the dock), and `Missed` off the list.
pub fn middle_press(
    state: &mut EditorState,
    layout: &ProjectsPanelLayout,
    point: Point2D,
) -> ProjectsPress {
    if !layout.contains_point(point) {
        return ProjectsPress::Missed;
    }
    let hit = layout.hit(point);
    if let Some(id) = hit.and_then(|target| open_document_at(state, target)) {
        state.editor_ui.pending_document_request = Some(DocumentTabRequest::Close(id));
    }
    ProjectsPress::Claimed(hit)
}

/// What a press meant to an open rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenamePress {
    /// No rename is open.
    NotEditing,
    /// The press is inside the field: the field keeps it (and the rename).
    InField,
    /// The press was elsewhere: a valid new name was committed, anything else
    /// cancelled, and the press goes on to whatever it landed on.
    Left,
}

/// Settle an open rename for a press at `point`. `layout` is the list's
/// layout when the dock shows one; without it no point is inside the field.
pub fn rename_press(
    state: &mut EditorState,
    layout: Option<&ProjectsPanelLayout>,
    point: Point2D,
) -> RenamePress {
    let panel = &state.editor_ui.projects_panel;
    let Some(edit) = panel.rename.as_ref() else {
        return RenamePress::NotEditing;
    };
    let in_field = layout.is_some_and(|layout| {
        layout.rows.iter().any(|row| {
            let crate::widgets::projects_panel_layout::RowKind::File(project, file) = row.kind
            else {
                return false;
            };
            panel
                .projects
                .get(project)
                .and_then(|view| view.files.get(file))
                .is_some_and(|view| {
                    view.path == edit.path && rename_field_rect(row, view).contains(point)
                })
        })
    });
    if in_field {
        return RenamePress::InField;
    }
    state.editor_ui.projects_panel.commit_or_cancel_rename();
    RenamePress::Left
}

/// What a press meant to an open "Move to Trash?" question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashPress {
    /// No question is open.
    NotAsking,
    /// The press is on the question's confirm or cancel button: it is theirs.
    OnButtons,
    /// The press was elsewhere and cancelled the question. `on_row` says it
    /// landed on the asking row itself, so nothing else should act on it.
    Cancelled { on_row: bool },
}

/// Settle an open "Move to Trash?" question for a press at `point`. A press on
/// its two buttons is left to the list; any other press cancels it (never
/// confirms: a destructive action needs the button).
pub fn settle_trash_confirm(
    state: &mut EditorState,
    layout: Option<&ProjectsPanelLayout>,
    point: Point2D,
) -> TrashPress {
    let panel = &state.editor_ui.projects_panel;
    let Some(path) = panel.trash_confirm.as_deref() else {
        return TrashPress::NotAsking;
    };
    let row = layout.and_then(|layout| {
        layout.rows.iter().find(|row| match row.kind {
            RowKind::File(project, file) => panel
                .projects
                .get(project)
                .and_then(|view| view.files.get(file))
                .is_some_and(|view| view.path == path),
            _ => false,
        })
    });
    let on = |button: Option<crate::Rect>| button.is_some_and(|rect| rect.contains(point));
    if row.is_some_and(|row| on(row.trash) || on(row.rename)) {
        return TrashPress::OnButtons;
    }
    let on_row = row.is_some_and(|row| row.rect.contains(point));
    state.editor_ui.projects_panel.cancel_trash_confirm();
    TrashPress::Cancelled { on_row }
}

/// Route a press at `point` at `now_ms`: record the request the row stands
/// for, start a rename (the pencil, or a second press on the same file within
/// [`DOUBLE_CLICK_MS`]) or a "Move to Trash?" question (the trash button), or
/// answer that question.
pub fn press(
    state: &mut EditorState,
    layout: &ProjectsPanelLayout,
    point: Point2D,
    now_ms: u64,
) -> ProjectsPress {
    let inside = layout.contains_point(point);
    if rename_press(state, inside.then_some(layout), point) == RenamePress::InField {
        return ProjectsPress::Claimed(None);
    }
    match settle_trash_confirm(state, inside.then_some(layout), point) {
        TrashPress::Cancelled { on_row: true } if inside => {
            return ProjectsPress::Claimed(None);
        }
        TrashPress::OnButtons => return answer_trash_question(state, layout, point),
        _ => {}
    }
    if !inside {
        return ProjectsPress::Missed;
    }
    let mut hit = layout.hit(point);
    let ui = &mut state.editor_ui.projects_panel;
    // A header's buttons only show while its header is hovered. A press that
    // never saw a move first (a synthesized click) cannot have aimed at a
    // button the user could not see, so it counts as a press on the header.
    if let Some(
        ProjectRowTarget::Plus(project)
        | ProjectRowTarget::Close(project)
        | ProjectRowTarget::Refresh(project),
    ) = hit
    {
        if !matches!(
            ui.hover,
            Some(
                ProjectRowTarget::Header(p)
                    | ProjectRowTarget::Plus(p)
                    | ProjectRowTarget::Close(p)
                    | ProjectRowTarget::Refresh(p)
            ) if p == project
        ) {
            hit = Some(ProjectRowTarget::Header(project));
        }
    }
    // The same for a file row's pencil and trash button.
    if let Some(
        ProjectRowTarget::RenameFile(project, file)
        | ProjectRowTarget::TrashFile(project, file)
        | ProjectRowTarget::CloseFile(project, file),
    ) = hit
    {
        if !matches!(
            ui.hover,
            Some(
                ProjectRowTarget::File(p, f)
                    | ProjectRowTarget::RenameFile(p, f)
                    | ProjectRowTarget::TrashFile(p, f)
                    | ProjectRowTarget::CloseFile(p, f)
            ) if (p, f) == (project, file)
        ) {
            hit = Some(ProjectRowTarget::File(project, file));
        }
    }
    // And for the close button of an open-document row.
    if let Some(ProjectRowTarget::CloseOpenDocument(tab)) = hit {
        if !matches!(
            ui.hover,
            Some(
                ProjectRowTarget::OpenDocument(t) | ProjectRowTarget::CloseOpenDocument(t)
            ) if t == tab
        ) {
            hit = Some(ProjectRowTarget::OpenDocument(tab));
        }
    }
    // And for a "New design here" button.
    if let Some(ProjectRowTarget::NewHere(project, folder)) = hit {
        let on_its_row = match (ui.hover, folder) {
            (Some(ProjectRowTarget::NewHere(p, f)), _) => (p, f) == (project, folder),
            (Some(ProjectRowTarget::Header(p) | ProjectRowTarget::Plus(p)), None) => p == project,
            (Some(ProjectRowTarget::Folder(p, f)), Some(first)) => (p, f) == (project, first),
            _ => false,
        };
        if !on_its_row {
            hit = Some(match folder {
                Some(first) => ProjectRowTarget::Folder(project, first),
                None => ProjectRowTarget::Header(project),
            });
        }
    }
    ui.hover = hit;
    let path = match hit {
        Some(
            ProjectRowTarget::File(project, file)
            | ProjectRowTarget::RenameFile(project, file)
            | ProjectRowTarget::TrashFile(project, file),
        ) => ui
            .projects
            .get(project)
            .and_then(|view| view.files.get(file))
            .map(|view| (view.path.clone(), is_renamable(view))),
        _ => None,
    };
    match (hit, path) {
        (Some(ProjectRowTarget::TrashFile(..)), Some((path, _))) => {
            ui.last_file_press = None;
            ui.begin_trash_confirm(&path);
        }
        (Some(ProjectRowTarget::RenameFile(..)), Some((path, true))) => {
            ui.last_file_press = None;
            ui.begin_rename(&path, now_ms);
        }
        (Some(ProjectRowTarget::File(..)), Some((path, true)))
            if ui.last_file_press.as_ref().is_some_and(|(last, at)| {
                *last == path && now_ms.saturating_sub(*at) < DOUBLE_CLICK_MS
            }) =>
        {
            ui.last_file_press = None;
            ui.begin_rename(&path, now_ms);
        }
        (Some(target), path) => {
            ui.last_file_press = path
                .filter(|(_, renamable)| *renamable && matches!(target, ProjectRowTarget::File(..)))
                .map(|(path, _)| (path, now_ms));
            if let Some(request) = document_request_for(state, target) {
                state.editor_ui.pending_document_request = Some(request);
            } else {
                let request = request_for(state, target);
                state.editor_ui.projects_panel.pending_request = request;
            }
        }
        (None, _) => ui.last_file_press = None,
    }
    ProjectsPress::Claimed(hit)
}

/// A press on the asking row's confirm (trash slot) or cancel (pencil slot)
/// button.
fn answer_trash_question(
    state: &mut EditorState,
    layout: &ProjectsPanelLayout,
    point: Point2D,
) -> ProjectsPress {
    let hit = layout.hit(point);
    let ui = &mut state.editor_ui.projects_panel;
    match hit {
        Some(ProjectRowTarget::TrashFile(..)) => {
            ui.confirm_trash();
        }
        _ => {
            ui.cancel_trash_confirm();
        }
    }
    ui.hover = hit;
    ProjectsPress::Claimed(hit)
}

/// Scroll the list just enough to bring the row of the file at `path` fully
/// into view. `false` when the file has no row in `layout` (a folded folder,
/// a collapsed project) or it is already visible.
pub fn reveal_file(state: &mut EditorState, layout: &ProjectsPanelLayout, path: &str) -> bool {
    let panel = &state.editor_ui.projects_panel;
    let Some(row) = layout.rows.iter().find(|row| match row.kind {
        RowKind::File(project, file) => panel
            .projects
            .get(project)
            .and_then(|view| view.files.get(file))
            .is_some_and(|view| view.path == path),
        _ => false,
    }) else {
        return false;
    };
    let top = layout.panel.origin.y;
    let bottom = top + layout.panel.size.y;
    let (row_top, row_bottom) = (row.rect.origin.y, row.rect.origin.y + row.rect.size.y);
    let delta = if row_top < top {
        row_top - top
    } else if row_bottom > bottom {
        row_bottom - bottom
    } else {
        return false;
    };
    let scroll = &mut state.editor_ui.projects_panel.scroll;
    scroll.offset = (layout.offset + delta).clamp(0.0, layout.max_scroll());
    true
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
#[path = "projects_open_tests.rs"]
mod projects_open_tests;
#[cfg(test)]
#[path = "projects_loose_tests.rs"]
mod projects_loose_tests;
#[cfg(test)]
#[path = "projects_panel_flow_tests.rs"]
pub(crate) mod projects_panel_flow_tests;
#[cfg(test)]
#[path = "projects_rename_tests.rs"]
mod projects_rename_tests;
#[cfg(test)]
#[path = "projects_trash_tests.rs"]
mod projects_trash_tests;
