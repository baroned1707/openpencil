//! "Revert Last Agent Run" through the real `DesktopApp`: tab switching, the
//! real launch hook and the real revert. Only the model is absent: a launch
//! points at a dead local port, and the test plays the agent by editing the
//! document itself.

use super::*;
use crate::recovery_host::test_support::{app_with_tabs, edit_active, entries_in, snapshot_name};
use op_editor_core::pen_node_ext::PenNodeExt;
use op_editor_core::{DocumentTabStatus, EditorState, LaunchRoute, NodeId};
use std::cell::Cell;

type Guard = std::sync::MutexGuard<'static, ()>;

fn lock() -> Guard {
    crate::agent_indicator_test_lock::LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Always answers the question with `choice`, counting how often it is asked.
fn answering(
    choice: Option<Choice>,
    asked: &Cell<usize>,
) -> impl FnMut(&str, &str) -> Option<Choice> + '_ {
    move |_title, _body| {
        asked.set(asked.get() + 1);
        choice
    }
}

/// Point the document on screen at a built-in provider that nothing listens on,
/// so a design launch starts a session without any network traffic of value.
fn configure_provider(app: &mut DesktopApp) {
    let state = app.host.editor_state_mut();
    state.editor_ui.agent_settings.builtin_agents.clear();
    let id = state.editor_ui.agent_settings.add_builtin_agent_config(
        "QA",
        "sk-test",
        "glm-5.3-flash",
        op_editor_core::BuiltinAgentKind::OpenAiCompat,
        "http://127.0.0.1:9/v1",
    );
    state.rebuild_chat_models();
    state.chat.selected_model = state
        .chat
        .available_models
        .iter()
        .position(|entry| entry.builtin_provider_id.as_deref() == Some(&id))
        .expect("the provider is selectable");
}

/// Send a design brief from the document on screen and launch it the way the
/// frame loop does. `true` when a run started.
fn send_brief(app: &mut DesktopApp) -> bool {
    configure_provider(app);
    let chat = &mut app.host.editor_state_mut().chat;
    chat.set_input_text("design a coffee event card");
    assert!(chat.begin_send());
    chat.launch_route = LaunchRoute::Orchestrator;
    app.launch_chat_if_pending() && app.runs.is_live()
}

/// A run launched in the document on screen and then stopped, as the user does.
fn start_and_stop_run(app: &mut DesktopApp) {
    assert!(send_brief(app), "the run must start");
    stop_run(app);
}

fn stop_run(app: &mut DesktopApp) {
    app.host.editor_state_mut().chat.pending_stop_chat = true;
    app.drain_stop_chat();
    assert!(!app.runs.is_live());
}

/// What the agent does to a document: an edit through the undo history.
fn agent_adds(app: &mut DesktopApp, name: &str) {
    let node: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "rectangle", "id": format!("agent-{name}"), "name": name,
        "x": 0, "y": 0, "width": 10, "height": 10
    }))
    .expect("node parses");
    let state = app.host.editor_state_mut();
    state.commit_history();
    state.active_children_mut().push(node);
}

fn names(app: &DesktopApp) -> Vec<String> {
    app.host
        .editor_state()
        .active_children()
        .iter()
        .filter_map(|node| node.base().name.clone())
        .collect()
}

fn toast_key(app: &DesktopApp) -> Option<String> {
    app.host
        .editor_state()
        .editor_ui
        .editor_toast
        .as_ref()
        .map(|toast| toast.i18n_key.clone())
}

fn revert_yes(app: &mut DesktopApp) -> Result<RevertOutcome, RevertError> {
    let asked = Cell::new(0);
    app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked))
}

/// A fresh app whose on-screen document is the blank starter, so a design
/// launch edits it synchronously (it clears the starter frame).
fn fresh_app() -> DesktopApp {
    let app = DesktopApp::new(None);
    assert!(
        op_editor_core::blank_starter::active_page_is_blank_starter(app.host.editor_state()),
        "fixture: the launch must have something to clear"
    );
    app
}

#[test]
fn checkpoint_is_captured_before_the_run_edits_the_document() {
    let _guard = lock();
    let mut app = fresh_app();
    let before = names(&app);
    assert_eq!(before.len(), 1, "the starter frame");

    assert!(send_brief(&mut app));

    assert!(
        names(&app).is_empty(),
        "the launch itself cleared the starter frame"
    );
    let checkpoint = app.runs.checkpoint.as_ref().expect("a checkpoint exists");
    let stored = checkpoint
        .snapshot()
        .doc
        .snapshot_find_node(0, &NodeId::new("n10"));
    assert!(
        stored.is_some(),
        "the checkpoint still has the frame the launch removed"
    );
    stop_run(&mut app);
}

#[test]
fn revert_restores_the_document_to_before_the_run() {
    let _guard = lock();
    let mut app = fresh_app();
    let before = names(&app);
    let before_name = app.host.editor_state().doc.name.clone();
    assert!(send_brief(&mut app));
    agent_adds(&mut app, "agent card");
    app.host.editor_state_mut().doc.name = Some("agent renamed".into());
    stop_run(&mut app);
    assert!(app.has_revertable_run());

    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));

    assert_eq!(names(&app), before);
    assert_eq!(app.host.editor_state().doc.name, before_name);
    assert!(
        app.host.editor_state().is_dirty(),
        "a revert is an edit to save"
    );
    assert!(app.runs.checkpoint.is_none(), "one revert per run");
    assert_eq!(app.runs.status, DocumentTabStatus::Idle);
    assert!(!app.has_revertable_run());
}

#[test]
fn revert_is_one_undo_step_and_undo_brings_the_agents_work_back() {
    let _guard = lock();
    let mut app = fresh_app();
    // The user's own work before the run, so there is history to protect.
    let user = app.host.editor_state_mut();
    user.commit_history();
    user.doc.name = Some("user work".into());
    assert!(send_brief(&mut app));
    agent_adds(&mut app, "agent card");
    agent_adds(&mut app, "agent card 2");
    stop_run(&mut app);
    let steps_before = app.host.editor_state().history.past.len();
    let with_agent = names(&app);

    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));
    assert_eq!(
        app.host.editor_state().history.past.len(),
        steps_before + 1,
        "exactly one new undo step"
    );

    assert!(app.host.apply_undo(), "Cmd+Z");
    assert_eq!(names(&app), with_agent, "the agent's work is back");
    assert!(app.host.editor_state().history.past.len() < steps_before + 1);
    // The steps from before the revert are still there to walk back through.
    assert!(app.host.editor_state().history.can_undo());
    assert!(app.host.apply_redo(), "and the revert can be redone");
    assert_eq!(
        app.host.editor_state().doc.name.as_deref(),
        Some("user work")
    );
    assert!(!names(&app).iter().any(|name| name.starts_with("agent")));
}

#[test]
fn revert_discards_user_edits_made_during_the_run_only_after_confirmation() {
    let _guard = lock();
    let mut app = fresh_app();
    assert!(send_brief(&mut app));
    stop_run(&mut app);
    // The user keeps working after the run.
    agent_adds(&mut app, "mine");
    let revision = app.host.editor_state().document_revision();

    let asked = Cell::new(0);
    let outcome = app.revert_last_agent_run_with(answering(Some(Choice::No), &asked));
    assert_eq!(outcome, Ok(RevertOutcome::Declined));
    assert_eq!(asked.get(), 1, "the user is asked exactly once");
    assert_eq!(names(&app), ["mine"], "nothing happened before the answer");
    assert_eq!(app.host.editor_state().document_revision(), revision);

    assert_eq!(
        app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked)),
        Ok(RevertOutcome::Reverted)
    );
    assert_eq!(asked.get(), 2);
    assert!(
        !names(&app).contains(&"mine".to_string()),
        "the edit is gone"
    );
    // ...and Cmd+Z brings it back, as the question promised.
    assert!(app.host.apply_undo());
    assert_eq!(names(&app), ["mine"]);
}

#[test]
fn the_question_names_the_discarded_work_and_the_undo() {
    let _guard = lock();
    let mut app = fresh_app();
    start_and_stop_run(&mut app);
    let seen = std::cell::RefCell::new(String::new());
    let _ = app.revert_last_agent_run_with(|title, body| {
        *seen.borrow_mut() = format!("{title}|{body}");
        Some(Choice::No)
    });
    let seen = seen.into_inner();
    assert!(seen.contains("Revert last agent run?"), "{seen}");
    assert!(
        seen.contains("edits you made since the run started"),
        "{seen}"
    );
    assert!(seen.contains("Cmd+Z"), "{seen}");
}

#[test]
fn declining_the_confirmation_changes_nothing() {
    let _guard = lock();
    let mut app = fresh_app();
    assert!(send_brief(&mut app));
    agent_adds(&mut app, "agent card");
    stop_run(&mut app);
    let revision = app.host.editor_state().document_revision();
    let steps = app.host.editor_state().history.past.len();
    let with_agent = names(&app);

    // No, and "the question could not be shown at all", both mean no.
    for answer in [Some(Choice::No), Some(Choice::Cancel), None] {
        let asked = Cell::new(0);
        let outcome = app.revert_last_agent_run_with(answering(answer, &asked));
        assert_eq!(outcome, Ok(RevertOutcome::Declined), "{answer:?}");
        assert_eq!(asked.get(), 1);
    }

    assert_eq!(names(&app), with_agent);
    assert_eq!(app.host.editor_state().document_revision(), revision);
    assert_eq!(app.host.editor_state().history.past.len(), steps);
    assert!(app.has_revertable_run(), "the checkpoint is kept for later");
}

#[test]
fn revert_is_refused_while_the_run_is_live() {
    let _guard = lock();
    let mut app = fresh_app();
    assert!(send_brief(&mut app));
    let revision = app.host.editor_state().document_revision();
    assert!(!app.has_revertable_run());

    let asked = Cell::new(0);
    let outcome = app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked));

    assert_eq!(outcome, Err(RevertError::RunIsLive));
    assert_eq!(asked.get(), 0, "refused before asking anything");
    assert_eq!(app.host.editor_state().document_revision(), revision);
    assert!(app.runs.is_live(), "the run is not stopped for the user");
    assert_eq!(
        toast_key(&app).as_deref(),
        Some("menu.revertAgentRun.runLive")
    );
    stop_run(&mut app);
    assert!(app.has_revertable_run(), "Stop keeps the checkpoint");
}

#[test]
fn revert_is_refused_without_a_checkpoint() {
    let _guard = lock();
    let mut app = fresh_app();
    let revision = app.host.editor_state().document_revision();

    let asked = Cell::new(0);
    let outcome = app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked));

    assert_eq!(outcome, Err(RevertError::NothingToRevert));
    assert_eq!(asked.get(), 0);
    assert_eq!(app.host.editor_state().document_revision(), revision);
    assert_eq!(
        toast_key(&app).as_deref(),
        Some("menu.revertAgentRun.nothing")
    );
}

#[test]
fn a_new_run_replaces_the_previous_checkpoint() {
    let _guard = lock();
    let mut app = fresh_app();
    start_and_stop_run(&mut app);
    agent_adds(&mut app, "first run result");
    let after_first_run = names(&app);

    start_and_stop_run(&mut app);
    agent_adds(&mut app, "second run result");
    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));

    assert_eq!(
        names(&app),
        after_first_run,
        "back to the start of the SECOND run, not the first"
    );
    assert!(!app.has_revertable_run(), "only one level");
}

#[test]
fn a_launch_that_does_not_start_a_run_keeps_the_previous_checkpoint() {
    let _guard = lock();
    let mut app = fresh_app();
    start_and_stop_run(&mut app);
    agent_adds(&mut app, "first run result");
    let kept = names(&app);
    let first = app.runs.checkpoint.as_ref().unwrap().snapshot().revision;

    // A send that ends in an error bubble reports a launch but starts no
    // session and edits nothing; a launch that reports nothing started anything.
    // Neither may replace the checkpoint of the earlier run.
    let candidate = RunCheckpoint::capture(&app.host);
    app.settle_run_checkpoint(Some(candidate), true);
    let candidate = RunCheckpoint::capture(&app.host);
    app.settle_run_checkpoint(Some(candidate), false);
    app.settle_run_checkpoint(None, true);
    assert_eq!(
        app.runs.checkpoint.as_ref().unwrap().snapshot().revision,
        first
    );

    // A launch that edited the document counts as a run even with no session.
    let candidate = RunCheckpoint::capture(&app.host);
    agent_adds(&mut app, "edited by the launch");
    app.settle_run_checkpoint(Some(candidate), true);
    assert_ne!(
        app.runs.checkpoint.as_ref().unwrap().snapshot().revision,
        first
    );
    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));
    assert_eq!(names(&app), kept);
}

#[test]
fn closing_or_replacing_the_document_drops_its_checkpoint() {
    let _guard = lock();
    // Replaced in place (MCP ReplaceDocument): same host epoch, new generation.
    let mut app = fresh_app();
    start_and_stop_run(&mut app);
    assert!(app.has_revertable_run());
    let replacement = EditorState::starter().doc;
    app.host.editor_state_mut().replace_document(replacement);
    assert!(!app.has_revertable_run());
    app.pump_document_runs();
    assert!(app.runs.checkpoint.is_none(), "the pump drops it");

    // Replaced by a whole new state (open / import): generation restarts at 0,
    // which the epoch alone tells apart.
    let mut app = fresh_app();
    start_and_stop_run(&mut app);
    let generation = app.host.editor_state().document_generation();
    assert!(app.host.install_imported_state(EditorState::starter()));
    assert_eq!(app.host.editor_state().document_generation(), generation);
    assert!(!app.has_revertable_run());
    let asked = Cell::new(0);
    assert_eq!(
        app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked)),
        Err(RevertError::DocumentReplaced)
    );
    assert_eq!(asked.get(), 0);
    assert!(app.runs.checkpoint.is_none());

    // Closed: the tab's runs go with it; the document that takes over has none.
    let (mut app, _paths, work, recovery) = app_with_tabs("closing", &["a.op", "b.op"]);
    start_and_stop_run(&mut app);
    assert!(app.has_revertable_run());
    let tabs = app.documents.len();
    assert!(app.close_document_tab_with(1, |_, _| Some(Choice::Yes)));
    assert_eq!(app.documents.len(), tabs - 1);
    assert!(!app.has_revertable_run());
    for dir in [work, recovery] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn checkpoint_never_touches_other_documents() {
    let _guard = lock();
    let (mut app, _paths, work, recovery) = app_with_tabs("others", &["a.op", "b.op"]);
    // Tab b (on screen) and tab a each get their own run and result.
    assert!(app.activate_document_slot(0));
    start_and_stop_run(&mut app);
    agent_adds(&mut app, "result in a");
    let names_a = names(&app);
    assert!(app.activate_document_slot(1));
    start_and_stop_run(&mut app);
    agent_adds(&mut app, "result in b");

    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));

    assert!(!names(&app).contains(&"result in b".to_string()));
    // Tab a is untouched, still has its checkpoint, and reverts on its own.
    assert!(app.activate_document_slot(0));
    assert_eq!(names(&app), names_a);
    assert!(app.has_revertable_run());
    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));
    assert!(!names(&app).contains(&"result in a".to_string()));
    for dir in [work, recovery] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn revert_of_a_parked_documents_run_waits_until_it_is_on_screen() {
    let _guard = lock();
    let (mut app, _paths, work, recovery) = app_with_tabs("parked", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0));
    assert!(send_brief(&mut app));
    agent_adds(&mut app, "result in a");
    // The user goes to tab b while a's run is still going, then it ends there.
    assert!(app.activate_document_slot(1));
    app.cancel_runs_of_slot(0);
    let parked_has_checkpoint = app
        .documents
        .slot_mut(0)
        .is_some_and(|slot| slot.local.runs.checkpoint.is_some());
    assert!(parked_has_checkpoint, "a Stop keeps the parked checkpoint");

    // The menu acts on the tab on screen only.
    assert!(!app.has_revertable_run());
    let asked = Cell::new(0);
    assert_eq!(
        app.revert_last_agent_run_with(answering(Some(Choice::Yes), &asked)),
        Err(RevertError::NothingToRevert)
    );
    assert_eq!(asked.get(), 0);

    assert!(app.activate_document_slot(0));
    assert!(app.has_revertable_run());
    assert!(names(&app).contains(&"result in a".to_string()));
    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));
    assert!(!names(&app).contains(&"result in a".to_string()));
    for dir in [work, recovery] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn checkpoint_does_not_duplicate_image_payloads() {
    let _guard = lock();
    let payload = format!("data:image/png;base64,{}", "A".repeat(500_000));
    let doc = jian_ops_schema::load_str(&format!(
        r#"{{"version":"1.0.0","children":[{{"type":"image","id":"img","src":"{payload}","width":10,"height":10}}]}}"#
    ))
    .expect("image document loads")
    .value;
    let mut app = DesktopApp::new(None);
    assert!(app
        .host
        .install_imported_state(EditorState::from_document(doc)));
    let src_ptr = |node: &jian_ops_schema::node::PenNode| match node {
        jian_ops_schema::node::PenNode::Image(image) => image.src.as_str().as_ptr(),
        other => panic!("not an image: {other:?}"),
    };
    let live = src_ptr(&app.host.editor_state().doc.children[0]);

    let checkpoint = RunCheckpoint::capture(&app.host);

    let stored = checkpoint
        .snapshot()
        .doc
        .snapshot_find_node(0, &NodeId::new("img"))
        .expect("the image node is in the checkpoint");
    assert_eq!(
        src_ptr(stored),
        live,
        "the 500 KB payload is shared, not copied"
    );
}

#[test]
fn menu_item_enabled_only_with_a_revertable_checkpoint() {
    let _guard = lock();
    let (mut app, _paths, work, recovery) = app_with_tabs("menu", &["a.op", "b.op"]);
    let mut states = vec![app.has_revertable_run()]; // b: nothing yet
    assert!(app.activate_document_slot(0));
    assert!(send_brief(&mut app));
    states.push(app.has_revertable_run()); // run live
    stop_run(&mut app);
    states.push(app.has_revertable_run()); // run ended
    assert!(app.activate_document_slot(1));
    states.push(app.has_revertable_run()); // another tab
    assert!(app.activate_document_slot(0));
    states.push(app.has_revertable_run()); // back
    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));
    states.push(app.has_revertable_run()); // reverted

    assert_eq!(states, [false, false, true, false, true, false]);
    // The refresh the frame loop runs is safe with no native menu (headless).
    app.refresh_revert_menu();
    for dir in [work, recovery] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn revert_takes_a_recovery_snapshot() {
    let _guard = lock();
    let (mut app, _paths, work, recovery) = app_with_tabs("snapshot", &["a.op"]);
    edit_active(&mut app, "before-the-agent");
    assert!(send_brief(&mut app));
    app.host.editor_state_mut().doc.name = Some("agent-name".into());
    app.host.editor_state_mut().mark_document_changed();
    stop_run(&mut app);
    // The periodic snapshot caught the agent's version.
    app.snapshot_document_now(
        app.documents.active_id(),
        crate::recovery_store::SnapshotReason::Autosnapshot,
    );
    assert_eq!(
        snapshot_name(&entries_in(&recovery)[0]).as_deref(),
        Some("agent-name")
    );

    assert_eq!(revert_yes(&mut app), Ok(RevertOutcome::Reverted));

    let entries = entries_in(&recovery);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        snapshot_name(&entries[0]).as_deref(),
        Some("before-the-agent"),
        "the reverted, unsaved document is on disk straight away"
    );
    for dir in [work, recovery] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
