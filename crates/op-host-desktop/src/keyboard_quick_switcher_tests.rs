//! The Cmd+K quick switcher on a headless `DesktopApp`: the chord, the modal
//! keys, and what choosing a row asks the host to do. The existing Cmd+digit /
//! Cmd+W chords must not change.

use super::*;
use op_editor_core::DocumentTabRequest;
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;
use winit::keyboard::{Key, NamedKey};

fn fixture_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-qs-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_document(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    save_to_path(&op_editor_core::EditorState::starter(), &path).expect("fixture saves");
    path
}

/// An app with documents `names` open (the last one on screen).
fn app_with(tag: &str, names: &[&str]) -> (DesktopApp, Vec<PathBuf>, PathBuf) {
    let dir = fixture_dir(tag);
    let mut app = DesktopApp::new(None);
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| write_document(&dir, name))
        .collect();
    for path in &paths {
        assert!(app.open_document_tab(path.clone()));
    }
    app.publish_document_tabs();
    (app, paths, dir)
}

fn press(app: &mut DesktopApp, key: Key, text: Option<&str>) {
    app.handle_key_pressed(&key, text);
}

fn cmd(app: &mut DesktopApp, letter: &str) {
    app.zoom_modifier = true;
    press(app, Key::Character(letter.into()), Some(letter));
    app.zoom_modifier = false;
}

fn type_text(app: &mut DesktopApp, text: &str) {
    for c in text.chars() {
        let s = c.to_string();
        press(app, Key::Character(s.as_str().into()), Some(&s));
    }
}

fn switcher_open(app: &DesktopApp) -> bool {
    app.host.quick_switcher_active()
}

fn active_label(app: &DesktopApp) -> String {
    app.host
        .editor_state()
        .editor_ui
        .document_tabs
        .iter()
        .find(|tab| tab.active)
        .map(|tab| tab.label.clone())
        .unwrap_or_default()
}

#[test]
fn cmd_k_opens_the_switcher_and_cmd_k_again_closes_it() {
    let (mut app, _paths, dir) = app_with("chord", &["a.op", "b.op"]);
    assert!(!switcher_open(&app));
    cmd(&mut app, "k");
    assert!(switcher_open(&app));
    cmd(&mut app, "k");
    assert!(!switcher_open(&app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cmd_k_does_nothing_while_a_text_field_has_focus() {
    let (mut app, _paths, dir) = app_with("focus", &["a.op", "b.op"]);
    app.host.editor_state_mut().chat.focused = true;
    cmd(&mut app, "k");
    assert!(!switcher_open(&app), "the chat input owns the keyboard");
    app.host.editor_state_mut().chat.focused = false;
    app.host.editor_state_mut().editor_ui.agent_settings_open = true;
    cmd(&mut app, "k");
    assert!(!switcher_open(&app), "a modal is up");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cmd_shift_k_and_cmd_alt_k_keep_their_own_meaning() {
    let (mut app, _paths, dir) = app_with("others", &["a.op"]);
    app.zoom_modifier = true;
    app.shift_modifier = true;
    press(&mut app, Key::Character("k".into()), Some("k"));
    assert!(!switcher_open(&app), "Cmd+Shift+K is the component browser");
    assert!(app.host.editor_state().editor_ui.component_browser_open);
    // Cmd+Alt+K is "create component"; it never opens the switcher either.
    app.shift_modifier = false;
    app.alt_modifier = true;
    press(&mut app, Key::Character("k".into()), Some("k"));
    assert!(!switcher_open(&app), "Cmd+Alt+K is create component");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn showing_home_closes_the_switcher_so_it_cannot_eat_the_next_click() {
    let (mut app, _paths, dir) = app_with("home", &["a.op", "b.op"]);
    cmd(&mut app, "k");
    assert!(switcher_open(&app));
    // A menu action (File > Home) shows Home without any key or press.
    app.host.editor_state_mut().editor_ui.home.visible = true;
    app.pump_projects();
    assert!(!switcher_open(&app));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn escape_closes_without_switching() {
    let (mut app, _paths, dir) = app_with("esc", &["a.op", "b.op"]);
    cmd(&mut app, "k");
    type_text(&mut app, "a");
    press(&mut app, Key::Named(NamedKey::Escape), None);
    assert!(!switcher_open(&app));
    assert_eq!(
        app.host.editor_state().editor_ui.pending_document_request,
        None
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn typing_goes_to_the_query_and_never_to_a_tool_shortcut() {
    let (mut app, _paths, dir) = app_with("typing", &["a.op", "b.op"]);
    let tool_before = app.host.editor_state().tool;
    cmd(&mut app, "k");
    type_text(&mut app, "rvt");
    assert_eq!(
        app.host
            .editor_state()
            .editor_ui
            .projects_panel
            .switcher
            .input
            .text(),
        "rvt"
    );
    assert_eq!(
        app.host.editor_state().tool,
        tool_before,
        "r / v / t are text here"
    );
    press(&mut app, Key::Named(NamedKey::Backspace), None);
    assert_eq!(
        app.host
            .editor_state()
            .editor_ui
            .projects_panel
            .switcher
            .input
            .text(),
        "rv"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn enter_on_an_open_document_switches_to_it() {
    let (mut app, _paths, dir) = app_with("enter-open", &["alpha.op", "bravo.op"]);
    assert_eq!(active_label(&app), "bravo.op");
    cmd(&mut app, "k");
    type_text(&mut app, "alpha");
    press(&mut app, Key::Named(NamedKey::Enter), None);
    assert!(!switcher_open(&app));
    app.pump_document_tabs();
    assert_eq!(active_label(&app), "alpha.op");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn enter_on_a_closed_project_file_opens_it() {
    let (mut app, _paths, dir) = app_with("enter-file", &["alpha.op", "bravo.op"]);
    let closed = write_document(&dir, "charlie.op");
    // Seed the dock's row as the host's scan would.
    {
        let ui = &mut app.host.editor_state_mut().editor_ui;
        ui.projects_panel.projects = vec![op_editor_core::ProjectView {
            root: dir.to_string_lossy().into_owned(),
            name: "qs".to_string(),
            files: vec![op_editor_core::ProjectFileView {
                path: closed.to_string_lossy().into_owned(),
                label: "charlie.op".to_string(),
                open_id: None,
                dirty: false,
                status: op_editor_core::DocumentTabStatus::Idle,
                active: false,
                folder: String::new(),
                kind: op_editor_core::ProjectFileKind::Design,
                missing: false,
                removable: false,
            }],
            ..op_editor_core::ProjectView::default()
        }];
    }
    cmd(&mut app, "k");
    type_text(&mut app, "charlie");
    press(&mut app, Key::Named(NamedKey::Enter), None);
    app.pump_projects();
    assert_eq!(active_label(&app), "charlie.op");
    assert_eq!(app.documents.len(), 3);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn other_chords_are_swallowed_while_it_is_open_and_the_clipboard_chords_reach_the_query() {
    let (mut app, _paths, dir) = app_with("swallow", &["a.op", "b.op"]);
    cmd(&mut app, "k");
    // Cmd+1 would switch tabs and Cmd+W close one; neither may act now.
    cmd(&mut app, "1");
    cmd(&mut app, "w");
    app.pump_document_tabs();
    assert_eq!(app.documents.len(), 2, "Cmd+W is swallowed");
    assert_eq!(active_label(&app), "b.op", "Cmd+1 is swallowed");
    // Select-all reaches the query.
    type_text(&mut app, "ab");
    cmd(&mut app, "a");
    let switcher = &app.host.editor_state().editor_ui.projects_panel.switcher;
    assert_eq!(switcher.input.highlight_range(), Some((0, 2)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cmd_digits_and_cmd_w_are_unchanged_when_the_switcher_is_closed() {
    let (mut app, _paths, dir) = app_with("shortcuts", &["a.op", "b.op", "c.op"]);
    assert_eq!(active_label(&app), "c.op");
    cmd(&mut app, "1");
    app.pump_document_tabs();
    assert_eq!(active_label(&app), "a.op", "Cmd+1");
    cmd(&mut app, "9");
    app.pump_document_tabs();
    assert_eq!(active_label(&app), "c.op", "Cmd+9 is the last tab");
    app.host
        .editor_state_mut()
        .editor_ui
        .pending_document_request = None;
    cmd(&mut app, "w");
    assert_eq!(
        app.host.editor_state().editor_ui.pending_document_request,
        Some(DocumentTabRequest::Close(
            app.host.editor_state().editor_ui.document_tabs[2].id
        )),
        "Cmd+W closes the document on screen"
    );
    let _ = std::fs::remove_dir_all(dir);
}
