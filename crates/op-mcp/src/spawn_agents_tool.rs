//! MCP tool `spawn_agents` — config validation plus an honest refusal.
//!
//! A design agent calls this to split a large design task into sub-tasks.
//! Each config item carries a prompt, the container node(s) to fill, and the
//! styleguide + guideline NAMES to pass along (sub-agents cannot search
//! styleguides, so the parent resolves and passes them in).
//!
//! This tool runs nothing. Sub-agents exist only inside the editor's built-in
//! design agent, which intercepts the call before it reaches this tool
//! (`op-host-desktop` `chat_session::handle_spawn_agents`, one sub-agent after
//! another). Every other connection — the live MCP server, headless `--mcp`,
//! ACP agents — dispatches here: a malformed config still reports its specific
//! argument error, and a valid one gets [`SPAWN_AGENTS_UNSUPPORTED`] as a tool
//! error, never a success that implies sections were built.

use std::collections::BTreeMap;

use serde_json::Value;

use super::spawn_agents_error::{SpawnConfigError, SpawnFieldError};
use super::{McpTool, ToolErrorCode, ToolOutcome};

/// Maximum number of sub-agents per call (Pencil recommends ≤ 8–10).
const MAX_AGENTS: usize = 12;

/// The text of the tool error a valid `spawn_agents` call gets outside the
/// editor's built-in design agent. It names the fallback so the caller can
/// continue without guessing.
pub const SPAWN_AGENTS_UNSUPPORTED: &str = "spawn_agents is available only inside the editor's \
built-in design agent; nothing was started. With this connection, build the sections yourself \
with batch_design (one section per call), then call finalize_design.";

// ---------------------------------------------------------------------------
// Public types (consumed by the desktop sub-agent runtime)
// ---------------------------------------------------------------------------

/// One spawned sub-agent specification — the caller's intent for one
/// design subtask. Public so the desktop sub-agent runtime can consume it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnSpec {
    /// The design prompt for this sub-agent.
    pub prompt: String,
    /// Node ids of containers the sub-agent should fill.
    pub container_nodes: Vec<String>,
    /// Name of the styleguide to pass to the sub-agent.
    pub styleguide_name: String,
    /// Guideline topic names to pass to the sub-agent.
    pub guideline_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// Parse helpers
// ---------------------------------------------------------------------------

/// Extract a required non-empty string field from a JSON object.
fn required_string(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, SpawnFieldError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(other) => Err(SpawnFieldError::NotAString {
            key: key.to_string(),
            value: other.to_string(),
        }),
        None => Err(SpawnFieldError::Required {
            key: key.to_string(),
        }),
    }
}

/// Extract an optional string-array field from a JSON object, defaulting to `[]`.
fn optional_string_array(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, SpawnFieldError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| match v {
                Value::String(s) => Ok(s.clone()),
                other => Err(SpawnFieldError::ElementNotAString {
                    key: key.to_string(),
                    index: i,
                    value: other.to_string(),
                }),
            })
            .collect(),
        Some(other) => Err(SpawnFieldError::NotAnArray {
            key: key.to_string(),
            value: other.to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Parse + validate
// ---------------------------------------------------------------------------

/// Parse and validate the `config` argument from the MCP args map.
///
/// Mirrors `batch_get::parse_patterns`: reads `args.get("config")` as a
/// JSON-string value and `serde_json::from_str`s it into a `Value` array.
/// Tests construct the args map directly (same convention as `batch_get_tests`):
/// `args.insert("config".into(), r#"[{"prompt":"...","styleguideName":"..."}]"#.into())`.
///
/// SURVIVING `String` SIGNATURE — this is a thin compat shim over
/// [`parse_spawn_config_typed`]. `op_host_desktop::sub_agent_session::parse_spawn_args`
/// returns this call as its tail expression from a `Result<_, String>` fn, so
/// no `From` coercion happens there and a typed return would break that crate.
/// Reach for `parse_spawn_config_typed` in new code.
pub fn parse_spawn_config(args: &BTreeMap<String, String>) -> Result<Vec<SpawnSpec>, String> {
    parse_spawn_config_typed(args).map_err(String::from)
}

/// The typed core of [`parse_spawn_config`]. In-crate callers use this;
/// `parse_spawn_config` is the `String`-compat shim above it.
pub fn parse_spawn_config_typed(
    args: &BTreeMap<String, String>,
) -> Result<Vec<SpawnSpec>, SpawnConfigError> {
    let raw = args.get("config").map(String::as_str).unwrap_or("").trim();

    if raw.is_empty() {
        return Err(SpawnConfigError::EmptyConfig);
    }

    let value: Value =
        serde_json::from_str(raw).map_err(|e| SpawnConfigError::ConfigNotJson(e.to_string()))?;

    let Value::Array(items) = value else {
        return Err(SpawnConfigError::ConfigNotArray);
    };

    if items.is_empty() {
        return Err(SpawnConfigError::EmptyConfig);
    }

    if items.len() > MAX_AGENTS {
        return Err(SpawnConfigError::TooManyAgents {
            count: items.len(),
            max: MAX_AGENTS,
        });
    }

    let mut specs = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let Value::Object(obj) = item else {
            return Err(SpawnConfigError::ItemNotObject { index });
        };

        let field = |cause| SpawnConfigError::Field { index, cause };

        let prompt = required_string(&obj, "prompt").map_err(field)?;
        if prompt.trim().is_empty() {
            return Err(SpawnConfigError::EmptyPrompt { index });
        }

        let styleguide_name = required_string(&obj, "styleguideName").map_err(field)?;
        if styleguide_name.trim().is_empty() {
            return Err(SpawnConfigError::EmptyStyleguideName { index });
        }

        let container_nodes = optional_string_array(&obj, "containerNodes").map_err(field)?;

        let guideline_names = optional_string_array(&obj, "guidelineNames").map_err(field)?;

        specs.push(SpawnSpec {
            prompt,
            container_nodes,
            styleguide_name,
            guideline_names,
        });
    }
    Ok(specs)
}

// ---------------------------------------------------------------------------
// Tool
// ---------------------------------------------------------------------------

/// Stateless MCP tool: validates the config, then refuses with
/// [`SPAWN_AGENTS_UNSUPPORTED`] because no sub-agent runtime exists behind an
/// MCP connection. Always registered so the catalog (and its pinned counts)
/// stays stable.
pub struct SpawnAgents;

impl McpTool for SpawnAgents {
    fn name(&self) -> &str {
        "spawn_agents"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        // Validation comes first so a malformed call still names its argument.
        if let Err(error) = parse_spawn_config_typed(args) {
            return ToolOutcome::Err(ToolErrorCode::InvalidArgument, error.to_string());
        }
        // No `Unsupported` code exists; `ToolFailed` is the closest: the call
        // was well-formed but this connection cannot perform it.
        ToolOutcome::Err(
            ToolErrorCode::ToolFailed,
            SPAWN_AGENTS_UNSUPPORTED.to_string(),
        )
    }
}

/// Snapshot constructor — mirrors the `get_guidelines_snapshot` convention.
/// This tool is stateless; no snapshot of `EditorState` is needed.
pub fn spawn_agents_snapshot() -> SpawnAgents {
    SpawnAgents
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an args map with `config` set to a JSON string, mirroring
    /// the `batch_get_tests` convention of constructing the map directly.
    fn args_with_config(config_json: &str) -> BTreeMap<String, String> {
        let mut args = BTreeMap::new();
        args.insert("config".into(), config_json.into());
        args
    }

    // --- parse_spawn_config tests ---

    #[test]
    fn parse_valid_single_item_returns_one_spec_with_all_fields_populated() {
        let args = args_with_config(
            r#"[{"prompt":"Design the hero","containerNodes":["n1","n2"],"styleguideName":"brand","guidelineNames":["web-app","mobile"]}]"#,
        );
        let specs = parse_spawn_config(&args).expect("valid single item");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].prompt, "Design the hero");
        assert_eq!(specs[0].container_nodes, vec!["n1", "n2"]);
        assert_eq!(specs[0].styleguide_name, "brand");
        assert_eq!(specs[0].guideline_names, vec!["web-app", "mobile"]);
    }

    #[test]
    fn parse_valid_two_item_config_returns_two_specs() {
        let args = args_with_config(
            r#"[
              {"prompt":"Hero section","styleguideName":"brand","containerNodes":["n10"],"guidelineNames":[]},
              {"prompt":"Footer section","styleguideName":"brand","containerNodes":["n20"],"guidelineNames":["web-app"]}
            ]"#,
        );
        let specs = parse_spawn_config(&args).expect("valid two items");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].prompt, "Hero section");
        assert_eq!(specs[1].prompt, "Footer section");
    }

    #[test]
    fn parse_item_without_optional_arrays_defaults_to_empty_vecs() {
        // containerNodes and guidelineNames are optional (#[serde(default)]).
        let args = args_with_config(r#"[{"prompt":"Fill nav","styleguideName":"brand"}]"#);
        let specs = parse_spawn_config(&args).expect("optional arrays absent");
        assert!(specs[0].container_nodes.is_empty());
        assert!(specs[0].guideline_names.is_empty());
    }

    #[test]
    fn parse_empty_config_array_returns_invalid_argument_error() {
        let args = args_with_config("[]");
        let err = parse_spawn_config(&args).unwrap_err();
        assert!(
            err.contains("non-empty config array"),
            "expected non-empty message, got: {err}"
        );
    }

    #[test]
    fn parse_missing_config_arg_returns_invalid_argument_error() {
        let err = parse_spawn_config(&BTreeMap::new()).unwrap_err();
        assert!(
            err.contains("non-empty config array"),
            "expected non-empty message, got: {err}"
        );
    }

    #[test]
    fn parse_item_with_empty_prompt_returns_invalid_argument_error() {
        let args = args_with_config(r#"[{"prompt":"  ","styleguideName":"brand"}]"#);
        let err = parse_spawn_config(&args).unwrap_err();
        assert!(
            err.contains("prompt must be non-empty"),
            "expected prompt error, got: {err}"
        );
    }

    #[test]
    fn parse_item_missing_styleguide_name_returns_invalid_argument_error() {
        // JSON without styleguideName at all — serde deserialises to empty string
        // (no default), so the field is required.
        // We supply it as empty to trigger the trim check.
        let args = args_with_config(r#"[{"prompt":"Hero","styleguideName":""}]"#);
        let err = parse_spawn_config(&args).unwrap_err();
        assert!(
            err.contains("styleguideName must be non-empty"),
            "expected styleguide error, got: {err}"
        );
    }

    #[test]
    fn parse_over_cap_config_returns_invalid_argument_error() {
        // Build 13 items (> MAX_AGENTS = 12).
        let items: Vec<String> = (0..13)
            .map(|i| format!(r#"{{"prompt":"Task {i}","styleguideName":"brand"}}"#))
            .collect();
        let json = format!("[{}]", items.join(","));
        let args = args_with_config(&json);
        let err = parse_spawn_config(&args).unwrap_err();
        assert!(
            err.contains("12-agent cap"),
            "expected cap error, got: {err}"
        );
    }

    // --- McpTool::call tests ---

    /// Invariant: a VALID config never gets a success. Nothing runs behind an
    /// MCP connection, so the call is a tool error that names the fallback.
    #[test]
    fn spawn_agents_over_mcp_returns_an_error_not_success() {
        let tool = spawn_agents_snapshot();
        for config in [
            r#"[{"prompt":"Hero","containerNodes":["n1"],"styleguideName":"brand","guidelineNames":["web-app"]}]"#,
            r#"[{"prompt":"Hero","styleguideName":"brand"},{"prompt":"Footer","styleguideName":"brand"}]"#,
        ] {
            match tool.call(&args_with_config(config)) {
                ToolOutcome::Err(ToolErrorCode::ToolFailed, msg) => {
                    assert_eq!(msg, SPAWN_AGENTS_UNSUPPORTED);
                    assert!(msg.contains("built-in design agent"), "{msg}");
                    assert!(msg.contains("batch_design"), "{msg}");
                    assert!(msg.contains("finalize_design"), "{msg}");
                    assert!(!msg.contains("agentIds"), "{msg}");
                }
                other => panic!("a valid config must be a ToolFailed error, got {other:?}"),
            }
        }
    }

    /// Invariant: validation runs before the refusal, so a malformed call
    /// still reports its specific argument error rather than the generic
    /// "unsupported" text.
    #[test]
    fn spawn_agents_validation_errors_still_name_the_argument() {
        let tool = spawn_agents_snapshot();
        let cases = [
            ("[]", "non-empty config array"),
            (
                r#"[{"prompt":"  ","styleguideName":"brand"}]"#,
                "prompt must be non-empty",
            ),
            (
                r#"[{"prompt":"Hero","styleguideName":""}]"#,
                "styleguideName must be non-empty",
            ),
            (r#"[{"prompt":"Hero"}]"#, "styleguideName"),
        ];
        for (config, expected) in cases {
            match tool.call(&args_with_config(config)) {
                ToolOutcome::Err(ToolErrorCode::InvalidArgument, msg) => {
                    assert!(msg.contains(expected), "{config}: {msg}");
                    assert_ne!(msg, SPAWN_AGENTS_UNSUPPORTED);
                }
                other => panic!("{config}: expected InvalidArgument, got {other:?}"),
            }
        }
    }

    #[test]
    fn call_empty_config_array_returns_invalid_argument() {
        let tool = spawn_agents_snapshot();
        let args = args_with_config("[]");
        match tool.call(&args) {
            ToolOutcome::Err(ToolErrorCode::InvalidArgument, msg) => {
                assert!(
                    msg.contains("non-empty config array"),
                    "error message: {msg}"
                );
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }

    #[test]
    fn call_item_with_empty_styleguide_name_returns_invalid_argument() {
        let tool = spawn_agents_snapshot();
        let args = args_with_config(r#"[{"prompt":"Hero","styleguideName":""}]"#);
        match tool.call(&args) {
            ToolOutcome::Err(ToolErrorCode::InvalidArgument, msg) => {
                assert!(
                    msg.contains("styleguideName must be non-empty"),
                    "error message: {msg}"
                );
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }
}
