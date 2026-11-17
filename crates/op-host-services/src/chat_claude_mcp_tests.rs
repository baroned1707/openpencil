use super::*;

const DOC_URL: &str = "http://127.0.0.1:3100/mcp/d/d2.abc123";
const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn open_endpoint() -> McpEndpoint {
    McpEndpoint::new(DOC_URL, None)
}

fn bound_dict(options: &ClaudeAgentOptions) -> &HashMap<String, McpServerConfig> {
    match &options.mcp_servers {
        McpServers::Dict(servers) => servers,
        other => panic!("expected an inline server dict, got {other:?}"),
    }
}

fn system_prompt_text(options: &ClaudeAgentOptions) -> String {
    match &options.system_prompt {
        Some(SystemPrompt::String(text)) => text.clone(),
        other => panic!("expected a string system prompt, got {other:?}"),
    }
}

#[test]
fn claude_turn_gets_the_launching_documents_mcp_endpoint() {
    let mut options = ClaudeAgentOptions::default();
    let guard = apply_document_mcp(&mut options, Some(&open_endpoint()));
    assert!(guard.is_none(), "an open endpoint needs no staged file");
    let servers = bound_dict(&options);
    assert_eq!(servers.len(), 1);
    let McpServerConfig::Http(http) = &servers[BOUND_SERVER_NAME] else {
        panic!("the bound server is an HTTP server");
    };
    assert_eq!(http.url, DOC_URL);
    assert_eq!(http.server_type, "http");
    // `null` headers make the CLI's schema reject the entry: always an object.
    assert_eq!(http.headers.as_ref().map(HashMap::len), Some(0));
}

#[test]
fn bound_endpoint_overrides_or_cannot_collide_with_the_global_openpencil_entry() {
    // `claude --help` states no precedence for a name defined both by
    // `--mcp-config` and by the user's config, so the bound server must never
    // reuse the name the integration writer gives the global entry.
    assert_ne!(BOUND_SERVER_NAME, "openpencil");
    let mut options = ClaudeAgentOptions::default();
    apply_document_mcp(&mut options, Some(&open_endpoint()));
    assert!(!bound_dict(&options).contains_key("openpencil"));
}

#[test]
fn other_mcp_servers_are_not_disabled_no_strict_flag() {
    let mut options = ClaudeAgentOptions::default();
    apply_document_mcp(&mut options, Some(&open_endpoint()));
    assert!(
        !options.strict_mcp_config,
        "--strict-mcp-config would switch off the user's Figma / GitHub servers"
    );
    assert!(options.disallowed_tools.is_empty());
    assert!(options.allowed_tools.is_empty());
}

#[test]
fn no_document_or_stopped_server_changes_nothing() {
    let untouched = format!("{:?}", ClaudeAgentOptions::default());
    let mut options = ClaudeAgentOptions::default();
    assert!(apply_document_mcp(&mut options, None).is_none());
    assert_eq!(format!("{options:?}"), untouched);
}

#[test]
fn caller_supplied_mcp_servers_are_left_alone() {
    let mut options = ClaudeAgentOptions {
        mcp_servers: McpServers::Path(PathBuf::from("/caller/mcp.json")),
        ..Default::default()
    };
    assert!(apply_document_mcp(&mut options, Some(&open_endpoint())).is_none());
    assert!(matches!(options.mcp_servers, McpServers::Path(_)));
    assert!(options.system_prompt.is_none());
}

#[test]
fn token_goes_in_a_header_never_in_the_url() {
    let endpoint = McpEndpoint::new(DOC_URL, Some(TOKEN.to_string()));
    let mut options = ClaudeAgentOptions::default();
    let guard = apply_document_mcp(&mut options, Some(&endpoint)).expect("a staged config file");

    // The SDK puts a `Dict` on the command line (visible to `ps`, logged at
    // debug level): a token-bearing config rides a file path instead.
    let McpServers::Path(path) = &options.mcp_servers else {
        panic!("a token must not ride the command line");
    };
    let text = std::fs::read_to_string(path).expect("config file");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let server = &json["mcpServers"][BOUND_SERVER_NAME];
    assert_eq!(server["url"], DOC_URL);
    assert!(!server["url"].as_str().unwrap().contains(TOKEN));
    assert!(!server["url"].as_str().unwrap().contains('?'));
    assert_eq!(
        server["headers"]["Authorization"],
        format!("Bearer {TOKEN}")
    );

    // Neither the options nor the endpoint print the token.
    assert!(!format!("{options:?}").contains(TOKEN));
    assert!(!format!("{endpoint:?}").contains(TOKEN));
    assert!(!system_prompt_text(&options).contains(TOKEN));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    let dir = path.parent().unwrap().to_path_buf();
    drop(guard);
    assert!(!dir.exists(), "the config dies with the turn");
}

#[test]
fn tool_name_references_match_the_registered_server_name() {
    let mut options = ClaudeAgentOptions::default();
    apply_document_mcp(&mut options, Some(&open_endpoint()));
    assert!(bound_dict(&options).contains_key(BOUND_SERVER_NAME));
    // Claude Code names an MCP tool `mcp__<server>__<tool>`.
    assert_eq!(
        bound_tool_prefix(),
        format!("mcp__{BOUND_SERVER_NAME}__"),
        "the prefix the agent sees is built from the registered name"
    );
    let prompt = system_prompt_text(&options);
    assert!(prompt.contains(&format!("{}*", bound_tool_prefix())));
    // The note appends to the turn's own prompt, never replaces it.
    let mut with_prompt = ClaudeAgentOptions {
        system_prompt: Some(SystemPrompt::String("You are a design assistant.".into())),
        ..Default::default()
    };
    apply_document_mcp(&mut with_prompt, Some(&open_endpoint()));
    let combined = system_prompt_text(&with_prompt);
    assert!(combined.starts_with("You are a design assistant."));
    assert!(combined.contains(&bound_tool_prefix()));
    // No allow-list exists that could name a server this turn does not have.
    assert!(options.allowed_tools.is_empty());
}
