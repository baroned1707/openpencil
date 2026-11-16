//! The bearer header in terminal-CLI configs. All writes go to a per-test temp
//! home — never the user's real client configs.

use super::*;
use auth::supports_bearer_header;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn temp_home(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("openpencil-mcp-auth-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create temp home");
    path
}

fn with_token(bearer: Option<&str>) -> LiveEndpoint<'_> {
    LiveEndpoint {
        port: 3100,
        lean: false,
        bearer,
    }
}

/// Where the `openpencil` entry's headers object lives in each JSON format.
fn headers_of(cli: McpCli, config: &Value) -> Option<&Value> {
    let entry = match cli {
        McpCli::OpenCode => config.pointer("/mcp/openpencil"),
        McpCli::ZCode => config.pointer("/mcp/servers/openpencil"),
        _ => config.pointer("/mcpServers/openpencil"),
    }?;
    entry.get("headers")
}

#[test]
fn integrations_write_the_auth_header_only_for_header_capable_clis_when_on() {
    let home = temp_home("matrix");
    for cli in McpCli::ALL {
        let path = set_cli_endpoint_at_home(cli, true, with_token(Some(TOKEN)), &home)
            .unwrap_or_else(|e| panic!("{} install: {e}", cli.label()));
        let text = fs::read_to_string(&path).expect("config written");
        let carries = text.contains(TOKEN);
        assert_eq!(
            carries,
            supports_bearer_header(cli),
            "{}: header present = {carries}\n{text}",
            cli.label()
        );
        if supports_bearer_header(cli) {
            match cli {
                McpCli::Codex => assert!(
                    text.contains(&format!(
                        "http_headers = {{ Authorization = \"Bearer {TOKEN}\" }}"
                    )),
                    "{text}"
                ),
                _ => {
                    let config: Value = serde_json::from_str(&text).expect("json config");
                    assert_eq!(
                        headers_of(cli, &config),
                        Some(&serde_json::json!({ "Authorization": format!("Bearer {TOKEN}") })),
                        "{}:\n{text}",
                        cli.label()
                    );
                }
            }
        }
        // The entry is still the live endpoint and still detected as enabled.
        assert!(
            text.contains("http://127.0.0.1:3100/mcp"),
            "{}",
            cli.label()
        );
        assert!(
            cli_config_has_openpencil(cli, &path) || cli == McpCli::Antigravity,
            "{} stays detected as enabled",
            cli.label()
        );

        // Turning the requirement off rewrites the entry without the header.
        set_cli_endpoint_at_home(cli, true, with_token(None), &home)
            .unwrap_or_else(|e| panic!("{} rewrite: {e}", cli.label()));
        let text = fs::read_to_string(&path).expect("config rewritten");
        assert!(
            !text.contains(TOKEN),
            "{} keeps no token:\n{text}",
            cli.label()
        );
    }
    let _ = fs::remove_dir_all(home);
}

#[test]
fn the_clis_that_cannot_carry_a_header_are_exactly_these_three() {
    let without: Vec<McpCli> = McpCli::ALL
        .into_iter()
        .filter(|cli| !supports_bearer_header(*cli))
        .collect();
    assert_eq!(without, [McpCli::GrokBuild, McpCli::ZCode, McpCli::Dsh]);
}

#[test]
fn an_unsupported_cli_is_left_exactly_as_the_open_install_writes_it() {
    let home = temp_home("unsupported");
    for cli in [McpCli::GrokBuild, McpCli::ZCode, McpCli::Dsh] {
        let path = set_cli_endpoint_at_home(cli, true, with_token(None), &home).expect("open");
        let open = fs::read_to_string(&path).expect("open config");
        set_cli_endpoint_at_home(cli, true, with_token(Some(TOKEN)), &home).expect("with token");
        assert_eq!(
            fs::read_to_string(&path).expect("config"),
            open,
            "{}: no header, no other change",
            cli.label()
        );
    }
    let _ = fs::remove_dir_all(home);
}

#[test]
fn adding_the_header_keeps_the_users_other_servers() {
    let home = temp_home("others");
    let config = home.join(".claude.json");
    fs::write(
        &config,
        r#"{"theme":"dark","mcpServers":{"other":{"type":"http","url":"https://example.com/mcp"}}}"#,
    )
    .expect("seed");
    set_cli_endpoint_at_home(McpCli::ClaudeCode, true, with_token(Some(TOKEN)), &home)
        .expect("install");
    let root: Value = serde_json::from_str(&fs::read_to_string(&config).expect("read")).unwrap();
    assert_eq!(root["theme"], "dark");
    assert_eq!(
        root["mcpServers"]["other"]["url"],
        "https://example.com/mcp"
    );
    assert_eq!(
        root["mcpServers"]["openpencil"]["headers"]["Authorization"],
        format!("Bearer {TOKEN}")
    );
    let _ = fs::remove_dir_all(home);
}

#[cfg(unix)]
#[test]
fn a_config_file_created_to_hold_a_token_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let home = temp_home("mode");
    let path = set_cli_endpoint_at_home(McpCli::Cursor, true, with_token(Some(TOKEN)), &home)
        .expect("install");
    let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let _ = fs::remove_dir_all(home);
}
