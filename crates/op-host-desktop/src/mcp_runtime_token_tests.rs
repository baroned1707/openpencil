//! `mcp_require_token` on a headless `DesktopApp`: the running live server
//! follows the setting, and the terminal integrations are rewritten when it
//! flips. The token lives in a temp home, never in the user's settings dir.

use crate::DesktopApp;
use op_editor_core::agent_settings::McpCli;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

fn temp_home(tag: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "openpencil-mcp-runtime-token-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("temp home");
    home
}

fn set_require_token(app: &mut DesktopApp, require: bool) {
    app.host
        .editor_state_mut()
        .editor_ui
        .agent_settings
        .mcp_require_token = require;
}

/// One raw HTTP POST to the live server; returns the whole response.
fn post(port: u16, headers: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("timeout");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

const PING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#;

#[test]
fn the_running_server_follows_the_require_token_setting() {
    let home = temp_home("server");
    let mut app = DesktopApp::new(None);
    app.mcp_integrations_home = Some(home.clone());
    set_require_token(&mut app, false);
    app.force_live_mcp_port = Some(0);
    app.reconcile_mcp_server_from_settings();
    let server = app.mcp_server.as_ref().expect("server");
    let port = server.port();
    let instance_token = server.token().to_string();

    // Off (the default): open access, the ping carries the instance token.
    assert!(!server.bearer_required());
    assert!(post(port, "", INITIALIZE).starts_with("HTTP/1.1 200"));
    assert!(post(port, "", PING).contains(&instance_token));

    // On: the next reconcile makes the server demand the stored token.
    set_require_token(&mut app, true);
    app.reconcile_mcp_server_from_settings();
    assert!(app.mcp_server.as_ref().expect("server").bearer_required());
    let token = std::fs::read_to_string(home.join(".openpencil").join("mcp-token"))
        .expect("the token was created beside the settings")
        .trim()
        .to_string();
    assert_eq!(token.len(), 64);
    assert!(post(port, "", INITIALIZE).starts_with("HTTP/1.1 401"));
    let bearer = format!("Authorization: Bearer {token}\r\n");
    assert!(post(port, &bearer, INITIALIZE).starts_with("HTTP/1.1 200"));
    let anonymous_ping = post(port, "", PING);
    assert!(
        !anonymous_ping.contains(&instance_token),
        "{anonymous_ping}"
    );

    // Off again: back to open access, no restart needed.
    set_require_token(&mut app, false);
    app.reconcile_mcp_server_from_settings();
    assert!(post(port, "", INITIALIZE).starts_with("HTTP/1.1 200"));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn flipping_require_token_rewrites_only_the_header_capable_integrations() {
    let home = temp_home("integrations");
    let mut app = DesktopApp::new(None);
    app.mcp_integrations_home = Some(home.clone());
    let claude = McpCli::ClaudeCode.index();
    let grok = McpCli::GrokBuild.index();
    {
        let settings = &mut app.host.editor_state_mut().editor_ui.agent_settings;
        settings.mcp_cli_enabled = [false; 13];
        settings.mcp_lean_profile = false;
        settings.mcp_require_token = false;
    }
    let state = |app: &DesktopApp| {
        let settings = &app.host.editor_state().editor_ui.agent_settings;
        Some((settings.mcp_cli_enabled, settings.mcp_server.port, false))
    };

    // Enable Claude Code and Grok Build, token not required.
    let before = state(&app);
    {
        let settings = &mut app.host.editor_state_mut().editor_ui.agent_settings;
        settings.mcp_cli_enabled[claude] = true;
        settings.mcp_cli_enabled[grok] = true;
    }
    assert!(!app.reconcile_mcp_cli_integrations(before));
    let claude_config = home.join(".claude.json");
    let grok_config = home.join(".grok").join("config.toml");
    assert!(!std::fs::read_to_string(&claude_config)
        .unwrap()
        .contains("Bearer"));
    let grok_open = std::fs::read_to_string(&grok_config).unwrap();

    // Require the token: Claude Code gains the header, Grok Build is untouched.
    let before = state(&app);
    set_require_token(&mut app, true);
    assert!(!app.reconcile_mcp_cli_integrations(before));
    let token = std::fs::read_to_string(home.join(".openpencil").join("mcp-token"))
        .expect("token file")
        .trim()
        .to_string();
    let claude_text = std::fs::read_to_string(&claude_config).unwrap();
    assert!(
        claude_text.contains(&format!("Bearer {token}")),
        "{claude_text}"
    );
    assert_eq!(std::fs::read_to_string(&grok_config).unwrap(), grok_open);

    // Drop the requirement: the header is removed again.
    let before = state(&app);
    set_require_token(&mut app, false);
    assert!(!app.reconcile_mcp_cli_integrations(before));
    assert!(!std::fs::read_to_string(&claude_config)
        .unwrap()
        .contains(&token));
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_server_started_while_the_token_is_required_demands_it_from_its_first_request() {
    let home = temp_home("start");
    let mut app = DesktopApp::new(None);
    app.mcp_integrations_home = Some(home.clone());
    set_require_token(&mut app, true);
    app.force_live_mcp_port = Some(0);
    app.reconcile_mcp_server_from_settings();
    let server = app.mcp_server.as_ref().expect("server");
    assert!(
        server.bearer_required(),
        "required from the moment it starts"
    );
    assert!(post(server.port(), "", INITIALIZE).starts_with("HTTP/1.1 401"));
    let _ = std::fs::remove_dir_all(home);
}
