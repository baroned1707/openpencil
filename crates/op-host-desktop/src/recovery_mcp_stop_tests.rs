//! `op stop` (the MCP `openpencil/shutdown` request) against a live server:
//! it must go through the same unsaved-changes prompts as the Quit menu item,
//! and a Cancel there must leave the editor running.

use super::multi_document_e2e_support::*;
use crate::message_dialog::Choice;
use crate::recovery_host::test_support::edit_active;

/// Send one token-authed shutdown request, answered by pumping the app.
fn send_shutdown(fx: &mut Fixture) -> Reply {
    let token = fx
        .app
        .mcp_server
        .as_ref()
        .expect("live MCP server")
        .token()
        .to_string();
    let body = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"openpencil/shutdown","params":{{"token":"{token}"}}}}"#
    );
    let mut replies = fx.drive(vec![
        Box::new(move |port| post_raw(port, "/mcp", &body)) as Job<Result<Reply, String>>
    ]);
    replies.remove(0).expect("HTTP exchange")
}

#[test]
fn op_stop_goes_through_confirm_close_and_cancel_keeps_running() {
    let mut fx = Fixture::new("op-stop");
    edit_active(&mut fx.app, "unsaved-in-c");
    let unsaved = fx.c;
    // No request, no question.
    assert!(!fx
        .app
        .mcp_quit_confirmed_with(|_, _| panic!("asked without a request")));

    // `op stop`, and the user cancels at the unsaved-changes prompt.
    assert!(!send_shutdown(&mut fx).is_error());
    let mut questions = Vec::new();
    let quit = fx.app.mcp_quit_confirmed_with(|_title, body| {
        questions.push(body.to_string());
        Some(Choice::Cancel)
    });
    assert!(!quit, "the editor keeps running");
    assert_eq!(questions.len(), 1, "the unsaved tab was asked about");
    assert!(questions[0].contains("c.op"), "{}", questions[0]);
    assert!(fx.is_dirty(unsaved), "the edit was not discarded");
    assert!(fx.app.mcp_server.is_some(), "the server is still up");
    // The request was dropped, not left pending to be asked on every loop turn.
    assert!(!fx
        .app
        .mcp_quit_confirmed_with(|_, _| panic!("asked a second time")));

    // A later `op stop` that the user answers with Don't Save does quit.
    assert!(!send_shutdown(&mut fx).is_error());
    assert!(fx.app.mcp_quit_confirmed_with(|_, _| Some(Choice::No)));
}

#[test]
fn op_stop_with_nothing_unsaved_quits_without_asking() {
    let mut fx = Fixture::new("op-stop-clean");
    assert!(!send_shutdown(&mut fx).is_error());

    assert!(fx
        .app
        .mcp_quit_confirmed_with(|_, _| panic!("nothing is unsaved")));
}
