//! Scene Template Center requests that replace the document never replace an
//! open one: they land in a pristine untitled tab or in a new tab.

use super::*;
use op_editor_core::blank_starter::active_page_is_blank_starter;
use op_editor_core::pen_node_ext::make_group;
use op_editor_core::EditorState;
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-template-tabs-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open_saved_tab(app: &mut DesktopApp, dir: &std::path::Path) -> PathBuf {
    let path = dir.join("work.op");
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    assert!(app.open_document_tab(path.clone()));
    path
}

fn request_generate(app: &mut DesktopApp, topic: &str) {
    let center = &mut app.host.editor_state_mut().editor_ui.scene_template_center;
    center.generate.set_text(topic);
    assert!(center.request_generate());
}

fn request_open(app: &mut DesktopApp) {
    app.host
        .editor_state_mut()
        .editor_ui
        .scene_template_center
        .request_open("slide-deck");
}

/// Block tab creation the way a running clipboard paste does.
fn block_tab_creation(app: &mut DesktopApp) {
    let (_tx, rx) = std::sync::mpsc::channel();
    app.pending_figma_paste = Some((0, rx));
}

#[test]
fn scene_template_generate_in_a_used_document_opens_a_new_tab() {
    let dir = temp_dir("generate");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    app.host.editor_state_mut().editor_ui.pinned_style_guide = Some("brand-guide".into());
    let baseline = app.host.editor_state().doc.clone();

    request_generate(&mut app, "Q3 review");
    assert!(app.drain_scene_template_generate());

    assert_eq!(app.documents.len(), 2, "the deck got its own tab");
    assert!(app.current_path.is_none(), "the new tab is untitled");
    assert!(app.host.editor_state().chat.pending_send.is_some());
    assert_eq!(
        app.host
            .editor_state()
            .editor_ui
            .pinned_style_guide
            .as_deref(),
        Some("brand-guide"),
        "the style pin follows the request"
    );
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    assert_eq!(app.host.editor_state().doc, baseline);
    assert!(
        app.host.editor_state().chat.is_untouched(),
        "no turn leaked"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn scene_template_generate_keeps_a_used_chat_in_its_own_tab() {
    let mut app = DesktopApp::new(None);
    app.host
        .editor_state_mut()
        .chat
        .input
        .set_text("earlier question");
    app.host.editor_state_mut().chat.begin_send();
    assert!(!app.active_document_is_pristine_untitled());

    request_generate(&mut app, "Q3 review");
    assert!(app.drain_scene_template_generate());

    assert_eq!(app.documents.len(), 2);
    assert!(app.activate_document_slot(0));
    assert!(
        !app.host.editor_state().chat.messages.is_empty(),
        "the conversation stayed where it was"
    );
}

#[test]
fn scene_template_generate_in_a_pristine_tab_reuses_it() {
    let mut app = DesktopApp::new(None);
    let tab = app.documents.active_id();

    request_generate(&mut app, "Q3 review");
    assert!(app.drain_scene_template_generate());

    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), tab);
    assert!(app.host.editor_state().chat.pending_send.is_some());
}

#[test]
fn scene_template_open_never_replaces_a_document_with_content_on_other_pages() {
    let mut app = DesktopApp::new(None);
    let state = app.host.editor_state_mut();
    assert!(state
        .add_page_with_name_and_children(
            Some("Work".into()),
            Some(vec![make_group("w".into(), "Hero", vec![])]),
        )
        .is_some());
    assert!(state.set_active_page(0));
    assert!(
        active_page_is_blank_starter(app.host.editor_state()),
        "the active page is blank, another page is not"
    );
    let baseline = app.host.editor_state().doc.clone();

    request_open(&mut app);
    assert!(app.drain_scene_template_open(0));

    assert_eq!(app.documents.len(), 2, "the template got its own tab");
    assert!(app.activate_document_slot(0));
    assert_eq!(app.host.editor_state().doc, baseline, "both pages intact");
}

#[test]
fn scene_template_open_in_a_saved_document_opens_a_new_tab() {
    let dir = temp_dir("open-saved");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    assert!(active_page_is_blank_starter(app.host.editor_state()));

    request_open(&mut app);
    assert!(app.drain_scene_template_open(0));

    assert_eq!(app.documents.len(), 2);
    assert!(
        app.current_path.is_none(),
        "the template is an unsaved document"
    );
    assert!(app.activate_document_slot(0));
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn scene_template_open_in_a_pristine_tab_reuses_it() {
    let mut app = DesktopApp::new(None);
    let tab = app.documents.active_id();

    request_open(&mut app);
    assert!(app.drain_scene_template_open(0));

    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.documents.active_id(), tab);
    assert!(!active_page_is_blank_starter(app.host.editor_state()));
}

#[test]
fn scene_template_open_still_appends_to_a_page_with_content() {
    let mut app = DesktopApp::new(None);
    assert!(app
        .host
        .editor_state_mut()
        .add_page_with_name_and_children(
            Some("Work".into()),
            Some(vec![make_group("w".into(), "Hero", vec![])]),
        )
        .is_some());

    request_open(&mut app);
    assert!(app.drain_scene_template_open(0));

    assert_eq!(app.documents.len(), 1, "appended in place, no tab");
}

#[test]
fn a_blocked_tab_refuses_scene_template_replacements_and_changes_nothing() {
    let dir = temp_dir("blocked");
    let mut app = DesktopApp::new(None);
    let original = open_saved_tab(&mut app, &dir);
    let baseline = app.host.editor_state().doc.clone();
    block_tab_creation(&mut app);

    request_generate(&mut app, "Q3 review");
    assert!(!app.drain_scene_template_generate());
    request_open(&mut app);
    assert!(app.drain_scene_template_open(0), "repaint for the notice");

    assert_eq!(app.documents.len(), 1);
    assert_eq!(app.current_path.as_deref(), Some(original.as_path()));
    assert_eq!(app.host.editor_state().doc, baseline);
    assert!(app.host.editor_state().chat.is_untouched());
    assert!(app.host.editor_state().editor_ui.editor_toast.is_some());
    let _ = std::fs::remove_dir_all(dir);
}
