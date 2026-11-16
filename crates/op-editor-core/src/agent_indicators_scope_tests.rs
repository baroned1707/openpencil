//! The indicator registry is scoped per document: a run in one document never
//! clears, tags or paints into another.

#![cfg(test)]

use crate::agent_indicators::*;

/// Leave the thread on the default scope and the registry empty.
struct Cleanup(&'static [u64]);

impl Drop for Cleanup {
    fn drop(&mut self) {
        set_current_scope(0);
        for scope in self.0 {
            drop_scope(*scope);
        }
    }
}

#[test]
fn a_run_in_another_scope_does_not_clear_this_scopes_indicators() {
    let _guard = test_guard();
    let _cleanup = Cleanup(&[901, 902]);
    set_current_scope(901);
    let epoch_a = begin();
    add_frame(epoch_a, "frame-a", "#FF0000", "Ava");

    set_current_scope(902);
    let epoch_b = begin();
    assert_ne!(epoch_a, epoch_b, "epochs are unique across scopes");
    assert!(snapshot().frames.is_empty(), "B starts clean");

    set_current_scope(901);
    let shown = snapshot();
    assert!(shown.run_active);
    assert!(shown.frames.contains_key("frame-a"));
    assert_eq!(active_epoch(), Some(epoch_a));
}

#[test]
fn an_epoch_finds_its_own_scope_whatever_scope_is_current() {
    let _guard = test_guard();
    let _cleanup = Cleanup(&[903, 904]);
    set_current_scope(903);
    let epoch_a = begin();
    set_current_scope(904);
    let _epoch_b = begin();

    // A worker for run A registers while B is the current scope.
    add_frame(epoch_a, "late-frame", "#FF0000", "Ava");
    confirm_cursor_agent(epoch_a, "#FF0000", "Ava");
    assert!(snapshot().frames.is_empty(), "B did not receive A's frame");
    assert!(cursor_agent_for_epoch(epoch_a).is_some());

    set_current_scope(903);
    assert!(snapshot().frames.contains_key("late-frame"));

    // Ending A retires its epoch only; B keeps running.
    end_if_epoch(epoch_a);
    add_frame(epoch_a, "after-end", "#FF0000", "Ava");
    assert!(!snapshot().frames.contains_key("after-end"));
    set_current_scope(904);
    assert!(snapshot().run_active);
}

#[test]
fn entering_a_scope_restores_the_previous_one() {
    let _cleanup = Cleanup(&[]);
    set_current_scope(7);
    {
        let _inside = enter_scope(8);
        assert_eq!(current_scope(), 8);
        {
            let _nested = enter_scope(9);
            assert_eq!(current_scope(), 9);
        }
        assert_eq!(current_scope(), 8);
    }
    assert_eq!(current_scope(), 7);
}

#[test]
fn dropping_a_scope_forgets_its_run() {
    let _guard = test_guard();
    let _cleanup = Cleanup(&[905]);
    set_current_scope(905);
    let epoch = begin();
    assert!(
        cursor_agent_for_epoch(epoch).is_none(),
        "no agent confirmed yet"
    );
    add_frame(epoch, "f", "#00FF00", "Gus");
    drop_scope(905);
    assert!(snapshot().frames.is_empty());
    add_frame(epoch, "g", "#00FF00", "Gus");
    assert!(
        snapshot().frames.is_empty(),
        "the epoch no longer names a run"
    );
}

#[test]
fn a_new_run_in_the_same_scope_replaces_the_old_one() {
    let _guard = test_guard();
    let _cleanup = Cleanup(&[906]);
    set_current_scope(906);
    let first = begin();
    add_frame(first, "f", "#00FF00", "Gus");
    let second = begin();
    assert!(
        snapshot().frames.is_empty(),
        "a new run in the same scope replaces the old one"
    );
    add_frame(first, "stale", "#00FF00", "Gus");
    assert!(
        snapshot().frames.is_empty(),
        "a replaced run's epoch is stale"
    );
    assert_eq!(active_epoch(), Some(second));
}
