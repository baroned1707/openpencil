//! A Claude Code chat turn is bound to the MCP endpoint of the document that
//! launched it, and nothing else is.

use super::mcp::LAST_CLAUDE_BINDING;
use super::providers::ChatLaunchContext;
use super::{chat_provider_for_selected_model, provider_for_selected_model};
use crate::DesktopApp;
use op_editor_core::{AgentSettings, DocumentId, EditorState};
use op_host_services::chat_claude::McpEndpoint;
use op_host_services::doc_io::save_to_path;
use std::path::PathBuf;

const TOKEN: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";

fn running_context(document: u64) -> ChatLaunchContext {
    ChatLaunchContext::default().with_document(Some(DocumentId(document)))
}

fn with_port(mut context: ChatLaunchContext, port: u16) -> ChatLaunchContext {
    context.mcp_port = Some(port);
    context
}

fn never_read() -> Option<std::io::Result<String>> {
    panic!("the token file must not be read when no token is required");
}

fn last_binding() -> Option<Option<McpEndpoint>> {
    LAST_CLAUDE_BINDING.with(|slot| slot.borrow_mut().take())
}

#[test]
fn lean_setting_selects_the_lean_route() {
    let full = with_port(running_context(3), 3100);
    let lean = ChatLaunchContext {
        mcp_lean: true,
        ..full.clone()
    };
    let full_url = full.claude_mcp_endpoint_with(never_read).expect("bound");
    let lean_url = lean.claude_mcp_endpoint_with(never_read).expect("bound");
    assert!(full_url
        .url()
        .starts_with("http://127.0.0.1:3100/mcp/d/d3."));
    assert!(!full_url.url().ends_with("/lean"));
    assert!(lean_url
        .url()
        .starts_with("http://127.0.0.1:3100/mcp/d/d3."));
    assert!(lean_url.url().ends_with("/lean"));
}

#[test]
fn settings_feed_the_context_only_while_the_server_runs() {
    let mut settings = AgentSettings::default();
    settings.mcp_server.port = 4100;
    settings.mcp_server.running = false;
    let stopped = ChatLaunchContext::default().with_mcp_settings(&settings);
    assert_eq!(stopped.mcp_port, None, "a stopped server has no endpoint");

    settings.mcp_server.running = true;
    settings.mcp_lean_profile = true;
    settings.mcp_require_token = true;
    let running = ChatLaunchContext::default().with_mcp_settings(&settings);
    assert_eq!(running.mcp_port, Some(4100));
    assert!(running.mcp_lean && running.mcp_require_token);
}

#[test]
fn no_document_or_stopped_server_changes_nothing() {
    assert_eq!(
        ChatLaunchContext::default().claude_mcp_endpoint_with(never_read),
        None
    );
    // A document but no running server.
    assert_eq!(
        running_context(1).claude_mcp_endpoint_with(never_read),
        None
    );
    // A running server but no document.
    let no_document = with_port(ChatLaunchContext::default(), 3100);
    assert_eq!(no_document.claude_mcp_endpoint_with(never_read), None);
    // `for_document` keeps its old shape: nothing bound until the host adds it.
    assert_eq!(
        ChatLaunchContext::for_document(None),
        ChatLaunchContext::default()
    );
}

#[test]
fn token_goes_in_a_header_never_in_the_url() {
    let context = ChatLaunchContext {
        mcp_require_token: true,
        ..with_port(running_context(2), 3100)
    };
    let endpoint = context
        .claude_mcp_endpoint_with(|| Some(Ok(TOKEN.to_string())))
        .expect("bound");
    assert!(endpoint.has_bearer());
    assert!(!endpoint.url().contains(TOKEN));
    assert!(!endpoint.url().contains('?'));
    assert!(!format!("{endpoint:?}").contains(TOKEN), "Debug redacts it");
    assert!(!format!("{context:?}").contains(TOKEN));

    // Required but unreadable: stay unbound (the global entry has its header)
    // rather than bind an endpoint that answers 401.
    assert_eq!(
        context.claude_mcp_endpoint_with(|| Some(Err(std::io::Error::other("denied")))),
        None
    );
    assert_eq!(context.claude_mcp_endpoint_with(|| None), None);
}

fn app_with_two_documents() -> (DesktopApp, PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "openpencil-claude-bind-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut app = DesktopApp::new(None);
    for name in ["a.op", "b.op"] {
        let path = dir.join(name);
        save_to_path(&EditorState::starter(), &path).expect("fixture saves");
        assert!(app.open_document_tab(path));
    }
    let settings = &mut app.host.editor_state_mut().editor_ui.agent_settings;
    settings.mcp_server.running = true;
    settings.mcp_server.port = 3100;
    (app, dir)
}

fn route_document(endpoint: &McpEndpoint) -> String {
    endpoint
        .url()
        .split("/mcp/d/")
        .nth(1)
        .expect("a document route")
        .split('/')
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn claude_turn_gets_the_launching_documents_mcp_endpoint() {
    let _guard = op_editor_core::agent_indicators::test_guard();
    let (mut app, dir) = app_with_two_documents();
    assert_eq!(app.host.editor_state().editor_ui.chat_selected_agent, 0);
    let on_screen = app.documents.active_id();
    let context = app.launch_context();
    assert_eq!(context.document, Some(on_screen));
    assert_eq!(context.mcp_port, Some(3100));

    last_binding();
    chat_provider_for_selected_model(&app.host, &context).expect("Claude chat provider");
    let bound = last_binding().flatten().expect("the chat turn is bound");
    assert_eq!(
        route_document(&bound),
        format!("d{}.{}", on_screen.0, bound_nonce(&bound))
    );
    assert!(bound.url().starts_with("http://127.0.0.1:3100/mcp/d/"));
    app.cancel_all_runs();
    let _ = std::fs::remove_dir_all(dir);
}

fn bound_nonce(endpoint: &McpEndpoint) -> String {
    route_document(endpoint)
        .split_once('.')
        .map(|(_, nonce)| nonce.to_string())
        .expect("the route carries the launch nonce")
}

#[test]
fn switching_tabs_mid_turn_does_not_rebind() {
    let _guard = op_editor_core::agent_indicators::test_guard();
    let (mut app, dir) = app_with_two_documents();
    let document_b = app.documents.active_id();
    let context_b = app.launch_context();

    // The user switches to the first tab while the turn launched from B runs.
    assert!(app.activate_document_slot(0));
    let document_a = app.documents.active_id();
    assert_ne!(document_a, document_b);
    assert_eq!(app.launch_context().document, Some(document_a));

    last_binding();
    chat_provider_for_selected_model(&app.host, &context_b).expect("provider");
    let bound = last_binding().flatten().expect("bound");
    assert!(
        route_document(&bound).starts_with(&format!("d{}.", document_b.0)),
        "the endpoint is the launching document's, not the one on screen: {}",
        bound.url()
    );
    app.cancel_all_runs();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn retry_keeps_the_original_documents_binding() {
    let _guard = op_editor_core::agent_indicators::test_guard();
    let (mut app, dir) = app_with_two_documents();
    let document_b = app.documents.active_id();
    let original = app.launch_context();

    last_binding();
    chat_provider_for_selected_model(&app.host, &original).expect("first send");
    let first = last_binding().flatten().expect("bound");

    // A retry rebuilds the provider from the context captured at launch, even
    // after the user has moved to another tab and back.
    assert!(app.activate_document_slot(0));
    chat_provider_for_selected_model(&app.host, &original).expect("retry");
    let retried = last_binding().flatten().expect("bound");
    assert_eq!(first, retried);
    assert!(route_document(&retried).starts_with(&format!("d{}.", document_b.0)));

    // A failed-subtask retry is a design run: it was never bound, so it stays
    // on the unbound constructor like the run it repeats.
    provider_for_selected_model(&app.host).expect("design provider");
    assert_eq!(last_binding(), Some(None));
    app.cancel_all_runs();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn classification_and_design_helpers_stay_unbound() {
    let _guard = op_editor_core::agent_indicators::test_guard();
    let (app, dir) = app_with_two_documents();
    let context = app.launch_context();
    assert!(context.claude_mcp_endpoint_with(never_read).is_some());

    last_binding();
    provider_for_selected_model(&app.host).expect("helper provider");
    assert_eq!(
        last_binding(),
        Some(None),
        "classification / design are unbound"
    );

    // Structural guard: no helper outside the chat turn asks for the chat
    // constructor, the only one that carries the document's endpoint.
    for (name, source) in [
        ("design_session.rs", include_str!("design_session.rs")),
        ("codegen_session.rs", include_str!("codegen_session.rs")),
        ("design_md_host.rs", include_str!("design_md_host.rs")),
        (
            "chat_session_launch_modify.rs",
            include_str!("chat_session_launch_modify.rs"),
        ),
        (
            "chat_session_launch_variants.rs",
            include_str!("chat_session_launch_variants.rs"),
        ),
        (
            "chat_session_launch_design.rs",
            include_str!("chat_session_launch_design.rs"),
        ),
        (
            "chat_session_launch_refine.rs",
            include_str!("chat_session_launch_refine.rs"),
        ),
    ] {
        assert!(
            !source.contains("chat_provider_for_selected_model"),
            "{name} must not build the document-bound chat provider"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}
