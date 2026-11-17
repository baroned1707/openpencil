//! Agent runs are bound to the document that started them.
//!
//! Every test drives real `DesktopApp` tab switching and the real per-frame
//! pump; only the model is replaced by channels the test feeds by hand.

use super::*;
use crate::message_dialog::Choice;
use crate::DesktopApp;
use op_ai::chat_provider::{ChatDelta, ChatToolResult, StopReason};
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::{ChatMessage, ChatRole, DocumentTabStatus, EditorState};
use op_editor_host_core::chat::ChatToolRequest;
use op_host_services::doc_io::save_to_path;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender};

/// The model side of one run, held by the test.
struct FakeRun {
    deltas: Sender<ChatDelta>,
    tools: Sender<ChatToolRequest>,
}

impl FakeRun {
    fn say(&self, text: &str) {
        self.deltas.send(ChatDelta::TextDelta(text.into())).unwrap();
    }

    fn finish(&self) {
        self.deltas
            .send(ChatDelta::Done {
                stop_reason: StopReason::EndTurn,
            })
            .unwrap();
    }

    /// Ask the host to insert a rectangle named `name`.
    fn insert(&self, name: &str) -> Receiver<ChatToolResult> {
        let args = format!(
            r##"{{"kind":"rect","name":"{name}","x":"5","y":"5","width":"50","height":"50","fill_hex":"#00ff00"}}"##
        );
        let (ack, ack_rx) = sync_channel(1);
        self.tools
            .send(ChatToolRequest {
                name: "insert_node".into(),
                args_json: args,
                ack,
            })
            .unwrap();
        ack_rx
    }

    fn is_cancelled(&self) -> bool {
        self.deltas
            .send(ChatDelta::TextDelta(String::new()))
            .is_err()
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-runs-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_document(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    save_to_path(&EditorState::starter(), &path).expect("fixture saves");
    path
}

/// An app with the given files open as tabs; the last one is on screen.
fn app_with_documents(tag: &str, names: &[&str]) -> (DesktopApp, Vec<PathBuf>, PathBuf) {
    let dir = temp_dir(tag);
    let mut app = DesktopApp::new(None);
    let paths: Vec<PathBuf> = names
        .iter()
        .map(|name| write_document(&dir, name))
        .collect();
    for path in &paths {
        assert!(app.open_document_tab(path.clone()), "{}", path.display());
    }
    (app, paths, dir)
}

/// Start a chat run in the document on screen, bound to its active chat tab.
fn start_run(app: &mut DesktopApp) -> FakeRun {
    let chat = &mut app.host.editor_state_mut().chat;
    chat.messages.push(ChatMessage::user("make a card"));
    chat.messages.push(ChatMessage::assistant_streaming());
    let tab = chat.active_index();
    let (deltas, delta_rx) = channel();
    let (tools, tool_rx) = channel();
    app.runs.current_chat = Some(ChatSession::from_channels(delta_rx, Some(tool_rx)));
    app.runs.chat_running_tab = Some(tab);
    FakeRun { deltas, tools }
}

fn names_in(state: &EditorState) -> Vec<String> {
    document_names(&state.doc)
}

fn document_names(doc: &jian_ops_schema::PenDocument) -> Vec<String> {
    doc.children
        .iter()
        .filter_map(|node| node.base().name.clone())
        .collect()
}

fn parked_names(app: &mut DesktopApp, index: usize) -> Vec<String> {
    let slot = app.documents.slot_mut(index).expect("slot exists");
    let parked = slot.parked.as_ref().expect("parked document");
    document_names(parked.scope().doc())
}

fn parked_assistant_text(app: &mut DesktopApp, index: usize) -> String {
    let slot = app.documents.slot_mut(index).expect("slot exists");
    let parked = slot.parked.as_ref().expect("parked document");
    parked
        .scope()
        .chat()
        .tabs()
        .iter()
        .flat_map(|tab| tab.messages.iter())
        .filter(|message| message.role == ChatRole::Assistant)
        .map(|message| message.content.clone())
        .collect()
}

fn active_assistant_text(app: &DesktopApp) -> String {
    app.host
        .editor_state()
        .chat
        .tabs()
        .iter()
        .flat_map(|tab| tab.messages.iter())
        .filter(|message| message.role == ChatRole::Assistant)
        .map(|message| message.content.clone())
        .collect()
}

fn status_at(app: &mut DesktopApp, index: usize) -> DocumentTabStatus {
    app.sync_active_slot();
    app.documents.tab_views(false, "")[index].status
}

#[test]
fn switching_tabs_with_a_live_run_is_allowed() {
    let _guard = agent_indicators::test_guard();
    let (mut app, paths, dir) = app_with_documents("switch", &["a.op", "b.op"]);
    let run = start_run(&mut app);
    let tab = app.runs.chat_running_tab;
    assert_eq!(
        app.document_switch_blocked(),
        None,
        "a live run is no reason"
    );

    assert!(app.activate_document_slot(0), "the switch goes through");
    assert_eq!(app.current_path.as_deref(), Some(paths[0].as_path()));
    assert!(!app.runs.is_live(), "A has no run of its own");
    assert!(app.slot_has_live_run(1), "B's run keeps living in its slot");

    assert!(app.activate_document_slot(1));
    assert!(app.runs.is_live());
    assert_eq!(
        app.runs.chat_running_tab, tab,
        "still bound to its chat tab"
    );
    drop(run);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn two_runs_in_two_documents_stream_independently() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("two", &["a.op", "b.op"]);
    let run_b = start_run(&mut app); // B is on screen
    assert!(app.activate_document_slot(0));
    let run_a = start_run(&mut app); // A is on screen, B parked

    let ack_a = run_a.insert("FromA");
    let ack_b = run_b.insert("FromB");
    run_a.say("alpha");
    run_b.say("beta");
    app.pump_document_runs();

    assert!(!ack_a.try_recv().expect("A's tool call ran").is_error);
    assert!(!ack_b.try_recv().expect("B's tool call ran").is_error);
    let on_screen = names_in(app.host.editor_state());
    assert!(on_screen.contains(&"FromA".to_string()), "{on_screen:?}");
    assert!(!on_screen.contains(&"FromB".to_string()), "{on_screen:?}");
    let parked = parked_names(&mut app, 1);
    assert!(parked.contains(&"FromB".to_string()), "{parked:?}");
    assert!(!parked.contains(&"FromA".to_string()), "{parked:?}");
    assert_eq!(active_assistant_text(&app), "alpha");
    assert_eq!(parked_assistant_text(&mut app, 1), "beta");

    run_a.finish();
    run_b.finish();
    app.pump_document_runs();
    assert!(!app.any_run_live());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn background_run_never_mutates_active() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("background", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    assert!(app.activate_document_slot(0)); // A on screen, B's run in the background
    let before = format!("{:?}", app.host.editor_state());
    let epoch = app.host.document_epoch();

    for round in 0..3 {
        let ack = run.insert(&format!("Bg{round}"));
        run.say("streaming");
        app.pump_document_runs();
        assert!(
            ack.try_recv().is_ok(),
            "the background tool call was served"
        );
    }

    assert_eq!(
        format!("{:?}", app.host.editor_state()),
        before,
        "the document on screen is exactly as it was"
    );
    assert_eq!(app.host.document_epoch(), epoch);
    assert_eq!(
        parked_names(&mut app, 1)
            .iter()
            .filter(|n| n.starts_with("Bg"))
            .count(),
        3
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn active_run_never_mutates_a_parked_document() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("active", &["a.op", "b.op"]);
    // B is on screen, A is parked and idle.
    let parked_before = {
        let slot = app.documents.slot_mut(0).unwrap();
        format!("{:?}", slot.parked.as_ref().unwrap().scope())
    };
    let run = start_run(&mut app);

    let ack = run.insert("OnlyInB");
    run.say("hello");
    app.pump_document_runs();

    assert!(ack.try_recv().is_ok());
    assert!(names_in(app.host.editor_state()).contains(&"OnlyInB".to_string()));
    let slot = app.documents.slot_mut(0).unwrap();
    assert_eq!(
        format!("{:?}", slot.parked.as_ref().unwrap().scope()),
        parked_before,
        "the parked document is untouched"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn starting_a_run_in_b_does_not_clear_as_indicators() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("indicators", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0)); // A on screen
    let epoch_a = agent_indicators::begin();
    agent_indicators::add_frame(epoch_a, "frame-a", "#FF0000", "Ava");

    assert!(app.activate_document_slot(1)); // B on screen
    assert!(
        agent_indicators::snapshot().frames.is_empty(),
        "A's glow is not painted over B"
    );
    let epoch_b = agent_indicators::begin();
    agent_indicators::add_frame(epoch_b, "frame-b", "#0000FF", "Bo");
    // A worker finishing a registration for A while B is showing lands in A.
    agent_indicators::add_frame(epoch_a, "frame-a2", "#FF0000", "Ava");
    assert!(!agent_indicators::snapshot().frames.contains_key("frame-a2"));

    assert!(app.activate_document_slot(0));
    let shown = agent_indicators::snapshot();
    assert!(shown.run_active, "A's run is still marked live");
    assert!(shown.frames.contains_key("frame-a"));
    assert!(shown.frames.contains_key("frame-a2"));
    assert!(!shown.frames.contains_key("frame-b"));
    assert_eq!(agent_indicators::active_epoch(), Some(epoch_a));
    agent_indicators::drop_scope(app.documents.active_id().0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn finished_run_in_parked_document_marks_needs_review() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("review", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    assert!(app.activate_document_slot(0));
    app.pump_document_runs();
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Running);

    run.finish();
    app.pump_document_runs();

    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::NeedsReview);
    assert_eq!(status_at(&mut app, 0), DocumentTabStatus::Idle);
    assert!(!app.any_run_live());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_run_finishing_on_screen_is_done_and_a_failed_one_is_an_error() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("done", &["a.op", "b.op"]);
    let run = start_run(&mut app);
    app.pump_document_runs();
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Running);
    run.finish();
    app.pump_document_runs();
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Done);

    // A run that dies with an error while its document is parked.
    let failing = start_run(&mut app);
    assert!(app.activate_document_slot(0));
    failing.say("error: provider rejected the request");
    failing.finish();
    app.pump_document_runs();
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Error);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn switching_to_a_needs_review_document_clears_it() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("seen", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    assert!(app.activate_document_slot(0));
    run.finish();
    app.pump_document_runs();
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::NeedsReview);

    assert!(app.activate_document_slot(1));
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Idle, "seen");
    assert!(app.activate_document_slot(0));
    assert_eq!(
        status_at(&mut app, 1),
        DocumentTabStatus::Idle,
        "stays seen"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_a_tab_with_a_live_run_cancels_it_after_confirm() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("close", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    assert!(app.activate_document_slot(0)); // B is parked with its run
    let mut asked = Vec::new();

    // Declined: nothing changes.
    let closed = app.close_document_tab_with(1, |_title, body| {
        asked.push(body.to_string());
        Some(Choice::Cancel)
    });
    assert!(!closed);
    assert_eq!(app.documents.len(), 2);
    assert!(app.slot_has_live_run(1));
    assert!(!run.is_cancelled());
    assert!(
        asked[0].contains("b.op"),
        "the question names the document: {}",
        asked[0]
    );

    // Confirmed: the run is cancelled and the tab goes.
    let closed = app.close_document_tab_with(1, |_title, _body| Some(Choice::Yes));
    assert!(closed);
    assert_eq!(app.documents.len(), 1);
    assert!(run.is_cancelled(), "the worker's channel is gone");
    assert!(!app.any_run_live());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn quit_mentions_live_runs() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("quit", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    assert!(app.activate_document_slot(0));
    let mut asked = Vec::new();

    let quit = app.confirm_close_with(|_title, body| {
        asked.push(body.to_string());
        Some(Choice::Cancel)
    });
    assert!(!quit, "Cancel keeps the app open");
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains("b.op"), "{}", asked[0]);
    assert!(app.slot_has_live_run(1), "and the run keeps going");

    let quit = app.confirm_close_with(|_title, _body| Some(Choice::Yes));
    assert!(quit);
    assert!(run.is_cancelled());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stop_acts_on_the_active_documents_run_only() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("stop", &["a.op", "b.op"]);
    let run_b = start_run(&mut app);
    assert!(app.activate_document_slot(0));
    let run_a = start_run(&mut app); // A on screen, B parked

    app.host.editor_state_mut().chat.pending_stop_chat = true;
    assert!(app.drain_stop_chat());

    assert!(!app.runs.is_live(), "the run on screen stopped");
    assert!(run_a.is_cancelled());
    assert!(app.slot_has_live_run(1), "the other document's run did not");
    assert!(!run_b.is_cancelled());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn chat_transcript_stays_with_its_document_across_switches() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("transcripts", &["a.op", "b.op"]);
    let run = start_run(&mut app);
    run.say("only for B");
    app.pump_document_runs();

    assert!(app.activate_document_slot(0));
    assert!(
        app.host.editor_state().chat.messages.is_empty(),
        "A has its own chat"
    );
    assert!(app.activate_document_slot(1));
    assert_eq!(active_assistant_text(&app), "only for B");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_cwd_comes_from_its_own_documents_project() {
    let dir = temp_dir("cwd");
    let first_project = dir.join("first");
    let second_project = dir.join("second");
    for project in [&first_project, &second_project] {
        std::fs::create_dir_all(project).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
    }
    let first = write_document(&first_project, "designs/home.op");
    let second = write_document(&second_project, "designs/app.op");
    let mut app = DesktopApp::new(None);
    assert!(app.open_document_tab(first));
    assert!(app.open_document_tab(second)); // on screen

    let in_second = app.launch_context();
    assert_eq!(
        in_second.project_cwd.map(|p| p.canonicalize().unwrap()),
        Some(second_project.canonicalize().unwrap())
    );
    assert!(app.activate_document_slot(0));
    let in_first = app.launch_context();
    assert_eq!(
        in_first.project_cwd.map(|p| p.canonicalize().unwrap()),
        Some(first_project.canonicalize().unwrap()),
        "a run started here works in THIS document's project"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn opening_a_file_keeps_a_chat_that_was_used_in_the_blank_tab() {
    let _guard = agent_indicators::test_guard();
    let dir = temp_dir("blank-chat");
    let file = write_document(&dir, "a.op");
    let mut app = DesktopApp::new(None);
    app.host
        .editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::user("a question that only chatted"));

    assert!(app.open_document_tab(file));

    assert_eq!(
        app.documents.len(),
        2,
        "the blank tab was kept, not replaced"
    );
    let kept = parked_chat_messages(&mut app, 0);
    assert_eq!(kept, vec!["a question that only chatted".to_string()]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_blank_tab_with_a_live_run_is_not_replaced() {
    let _guard = agent_indicators::test_guard();
    let dir = temp_dir("blank-run");
    let file = write_document(&dir, "a.op");
    let mut app = DesktopApp::new(None);
    let run = start_run(&mut app);

    assert!(app.open_document_tab(file));

    assert_eq!(app.documents.len(), 2);
    assert!(app.slot_has_live_run(0), "the run still has its document");
    assert!(!run.is_cancelled());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_stop_raised_before_switching_is_served_by_the_document_it_was_raised_in() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("leaving", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    app.host.editor_state_mut().chat.pending_stop_chat = true; // not drained yet

    assert!(app.activate_document_slot(0));

    assert!(run.is_cancelled(), "B's run was stopped, not parked live");
    assert!(!app.slot_has_live_run(1));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn closing_a_dirty_tab_stops_its_run_only_after_the_save_prompt() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("dirty-run", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    app.host.editor_state_mut().mark_document_changed();
    assert!(app.activate_document_slot(0));
    let mut run_was_stopped_at_save_prompt = None;
    let mut question = 0;

    let closed = app.close_document_tab_with(1, |_title, _body| {
        question += 1;
        if question == 1 {
            return Some(Choice::Yes); // stop the agent
        }
        run_was_stopped_at_save_prompt = Some(run.is_cancelled());
        Some(Choice::No) // discard the unsaved edits
    });

    assert!(closed);
    assert_eq!(
        run_was_stopped_at_save_prompt,
        Some(false),
        "the run is still alive while the save prompt is open"
    );
    assert!(run.is_cancelled(), "and stopped once the close went ahead");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancelling_a_save_prompt_leaves_agent_runs_alive() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("cancel-save", &["a.op", "b.op"]);
    let run = start_run(&mut app); // in B
    app.host.editor_state_mut().mark_document_changed(); // B has unsaved edits
    assert!(app.activate_document_slot(0)); // B is parked with its run
    assert!(!app.document_is_dirty() && app.slot_has_live_run(1));

    // Quit: agree to stop the agent, then Cancel at the save prompt.
    let mut question = 0;
    let quit = app.confirm_close_with(|_title, _body| {
        question += 1;
        Some(if question == 1 {
            Choice::Yes
        } else {
            Choice::Cancel
        })
    });
    assert!(!quit, "Cancel keeps the app open");
    assert_eq!(question, 2, "the agent question, then the save prompt");
    assert!(
        app.slot_has_live_run(1),
        "the run survived the cancelled quit"
    );
    assert!(!run.is_cancelled(), "its worker is untouched");

    // Closing the tab behaves the same way.
    let mut question = 0;
    let closed = app.close_document_tab_with(1, |_title, _body| {
        question += 1;
        Some(if question == 1 {
            Choice::Yes
        } else {
            Choice::Cancel
        })
    });
    assert!(!closed);
    assert_eq!(app.documents.len(), 2);
    assert!(app.slot_has_live_run(1));
    assert!(!run.is_cancelled());

    // Declining to stop the agent leaves it running too.
    assert!(!app.confirm_close_with(|_title, _body| Some(Choice::Cancel)));
    assert!(app.slot_has_live_run(1));

    // Finally, Don't Save: now the quit goes ahead and the run is stopped.
    let mut question = 0;
    assert!(app.confirm_close_with(|_title, _body| {
        question += 1;
        Some(if question == 1 {
            Choice::Yes
        } else {
            Choice::No
        })
    }));
    assert!(run.is_cancelled());
    let _ = std::fs::remove_dir_all(dir);
}

fn parked_chat_messages(app: &mut DesktopApp, index: usize) -> Vec<String> {
    let slot = app.documents.slot_mut(index).expect("slot exists");
    let parked = slot.parked.as_ref().expect("parked document");
    parked
        .scope()
        .chat()
        .tabs()
        .iter()
        .flat_map(|tab| tab.messages.iter())
        .map(|message| message.content.clone())
        .collect()
}

/// A run belongs to its document by id, not by file name: renaming the file
/// from the Projects dock, while the document is on screen or parked, neither
/// stops the run nor loses what it writes.
#[test]
fn agent_run_survives_the_rename() {
    let _guard = agent_indicators::test_guard();
    let dir = temp_dir("rename").canonicalize().unwrap();
    // A project marker, so the dock lists (and may rename) these files.
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    let mut app = DesktopApp::new(None);
    let paths: Vec<PathBuf> = ["a.op", "b.op"]
        .iter()
        .map(|name| write_document(&dir, name))
        .collect();
    for path in &paths {
        assert!(app.open_document_tab(path.clone()));
    }
    app.pump_projects();
    let run = start_run(&mut app); // in B, on screen
    let id = app.documents.active_id();

    // On screen: rename B's own file.
    assert_eq!(
        app.rename_design_file(&paths[1], "renamed"),
        Ok(()),
        "a live run is no reason to refuse"
    );
    let renamed = dir.join("renamed.op");
    assert_eq!(app.current_path.as_deref(), Some(renamed.as_path()));
    assert_eq!(app.documents.active_id(), id, "the same document");
    assert!(app.runs.is_live(), "the run did not stop");
    let ack = run.insert("AfterRename");
    run.say("still working");
    app.pump_document_runs();
    assert!(!ack.try_recv().expect("the tool call ran").is_error);
    assert!(names_in(app.host.editor_state()).contains(&"AfterRename".to_string()));
    assert_eq!(active_assistant_text(&app), "still working");

    // Parked: A is on screen, B (with its run) in the background.
    assert!(app.activate_document_slot(0));
    assert!(app.slot_has_live_run(1));
    assert_eq!(app.rename_design_file(&renamed, "again"), Ok(()));
    assert_eq!(
        app.documents.slot_paths()[1].as_deref(),
        Some(dir.join("again.op").as_path())
    );
    assert!(app.slot_has_live_run(1), "the parked run did not stop");
    let ack = run.insert("WhileParked");
    app.pump_document_runs();
    assert!(
        !ack.try_recv()
            .expect("the background tool call ran")
            .is_error
    );
    assert!(parked_names(&mut app, 1).contains(&"WhileParked".to_string()));
    run.finish();
    app.pump_document_runs();
    assert!(!app.any_run_live());
    let _ = std::fs::remove_dir_all(dir);
}

#[path = "agent_run_autosave_tests.rs"]
mod autosave_tests;
