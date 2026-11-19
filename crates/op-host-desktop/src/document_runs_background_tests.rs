//! Background work keeps moving when the window does not repaint, and several
//! runs finishing at once never move the user's screen. Nested in
//! `document_runs_tests` for its fake-run fixtures.

use super::*;
use crate::document_runs_host::background_pump::{wake_pump_due, REDRAW_STALE_AFTER};
use std::time::{Duration, Instant};

fn aged(by: Duration) -> Option<Instant> {
    Some(Instant::now() - by)
}

#[test]
fn the_wake_path_pumps_only_with_work_and_only_when_no_redraw_did() {
    let now = Instant::now();
    assert!(wake_pump_due(None, now, true), "nothing has pumped yet");
    assert!(!wake_pump_due(None, now, false), "no work");
    assert!(
        !wake_pump_due(Some(now), now, true),
        "a redraw just pumped: no double pump"
    );
    assert!(!wake_pump_due(
        Some(now),
        now + REDRAW_STALE_AFTER - Duration::from_millis(1),
        true
    ));
    assert!(
        wake_pump_due(Some(now), now + REDRAW_STALE_AFTER, true),
        "redraws have stopped"
    );
}

#[test]
fn an_occluded_window_still_advances_a_parked_documents_run() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("occluded", &["a.op", "b.op"]);
    assert!(app.activate_document_slot(0));
    let run = start_run(&mut app);
    assert!(app.activate_document_slot(1));
    assert!(app.has_background_work());

    // No redraw ever comes: only the wake path runs.
    app.last_background_pump = None;
    let ack = run.insert("FromWake");
    assert!(app.pump_background_when_redraw_starved().is_some());
    assert!(!ack.try_recv().expect("the tool call ran").is_error);
    assert!(parked_names(&mut app, 0).contains(&"FromWake".to_string()));

    // The same stretch of time: a second wake finds the fresh stamp.
    let second = run.insert("TooSoon");
    assert_eq!(app.pump_background_when_redraw_starved(), None);
    assert!(second.try_recv().is_err(), "not pumped twice in a row");

    // Redraws are still missing 200 ms later: the wake pumps again.
    app.last_background_pump = aged(Duration::from_millis(200));
    assert!(app.pump_background_when_redraw_starved().is_some());
    assert!(parked_names(&mut app, 0).contains(&"TooSoon".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_redraws_stamp_silences_the_wake_path() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("stamped", &["a.op", "b.op"]);
    let _run = start_run(&mut app);
    app.note_background_pump();
    assert_eq!(app.pump_background_when_redraw_starved(), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// What the user sees of the document on screen.
fn screen_trace(
    app: &mut DesktopApp,
) -> (String, String, String, u64, Vec<op_editor_core::DocumentId>) {
    let state = app.host.editor_state();
    (
        serde_json::to_string(&state.doc).unwrap(),
        format!("{:?}", state.selection),
        format!("{:?}", state.viewport),
        app.host.document_epoch(),
        (0..app.documents.len())
            .map(|index| app.documents.id_at(index).unwrap())
            .collect(),
    )
}

#[test]
fn several_runs_finishing_in_one_pump_never_switch_the_screen() {
    let _guard = agent_indicators::test_guard();
    let (mut app, _paths, dir) = app_with_documents("finish-many", &["a.op", "b.op", "c.op"]);
    // Runs in `a` and `b`; the user ends up looking at `c`.
    assert!(app.activate_document_slot(0));
    let run_a = start_run(&mut app);
    assert!(app.activate_document_slot(1));
    let run_b = start_run(&mut app);
    assert!(app.activate_document_slot(2));
    app.host
        .editor_state_mut()
        .set_single_selection(op_editor_core::NodeId::new("n10"));
    app.host.editor_state_mut().viewport.pan_x = 41.0;
    app.pump_document_runs();
    let on_screen = app.documents.active_id();
    let before = screen_trace(&mut app);

    // One run succeeds, the other dies with an error, in the same frame.
    run_a.insert("Fresh");
    run_a.finish();
    run_b.say("error: provider rejected the request");
    run_b.finish();
    app.pump_document_runs();

    assert_eq!(
        app.documents.active_id(),
        on_screen,
        "the screen stayed put"
    );
    assert_eq!(screen_trace(&mut app), before, "c is exactly as it was");
    assert_eq!(status_at(&mut app, 0), DocumentTabStatus::NeedsReview);
    assert_eq!(status_at(&mut app, 1), DocumentTabStatus::Error);
    assert_eq!(status_at(&mut app, 2), DocumentTabStatus::Idle);
    assert!(parked_names(&mut app, 0).contains(&"Fresh".to_string()));
    assert!(!app.any_run_live());
    let _ = std::fs::remove_dir_all(dir);
}
