//! `get_guidelines` `category: "data"` — look a product type up in the
//! embedded design-data table (palette, typography, layout pattern, rules,
//! anti-patterns), or, with `kind: "style"`, a style definition (what it is
//! best and not for, effects, accessibility). `kind` defaults to `product`.
//!
//! The result is TOOL RESULT content: one compact JSON document built by
//! `op_ai_skills::design_data_json` / `style_data_json`, with every string from
//! the table as a JSON string value (so a hostile string cannot change the structure) and
//! the whole text capped at 6 KB. It is reference data, never part of a
//! system prompt. External MCP agents and the in-app design loop reach this
//! same function through `GetGuidelines::call`.

use std::collections::BTreeMap;

use op_ai_skills::design_data::PLATFORMS;
use op_ai_skills::design_data_json;
use op_ai_skills::design_data_search::MAX_LIMIT;
use op_ai_skills::style_data_json;

use super::{ToolErrorCode, ToolOutcome};

const DEFAULT_LIMIT: usize = 1;

/// What `category: "data"` searches.
const KINDS: [&str; 2] = ["product", "style"];

pub(crate) fn call_data(args: &BTreeMap<String, String>) -> ToolOutcome {
    let kind = match args
        .get("kind")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        None => "product".to_string(),
        Some(value) if KINDS.contains(&value.as_str()) => value,
        Some(other) => {
            return ToolOutcome::Err(
                ToolErrorCode::InvalidArgument,
                format!("kind must be one of {}, got {other:?}", KINDS.join(", ")),
            )
        }
    };
    let is_style = kind == "style";
    let query =
        match args.get("query").map(|value| value.trim()) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolOutcome::Err(
                ToolErrorCode::MissingArgument,
                if is_style {
                    "query is required for kind=\"style\": a style name, e.g. \"glassmorphism\" \
                     or the name after external: in a product match."
                } else {
                    "query is required for category=\"data\": a few words naming the product type, \
                     e.g. \"fintech dashboard\"."
                }
                .to_string(),
            ),
        };
    // The platform filter belongs to product records only.
    let platform = match args
        .get("platform")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        _ if is_style => None,
        Some(value) if PLATFORMS.contains(&value.as_str()) => Some(value),
        Some(other) => {
            return ToolOutcome::Err(
                ToolErrorCode::InvalidArgument,
                format!(
                    "platform must be one of {}, got {other:?}",
                    PLATFORMS.join(", ")
                ),
            )
        }
        None => None,
    };
    let limit = match args
        .get("limit")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        None => DEFAULT_LIMIT,
        Some(raw) => match raw.parse::<usize>() {
            Ok(value) if value >= 1 => value.min(MAX_LIMIT),
            _ => {
                return ToolOutcome::Err(
                    ToolErrorCode::InvalidArgument,
                    format!("limit must be a whole number from 1 to {MAX_LIMIT}, got {raw:?}"),
                )
            }
        },
    };
    if is_style {
        return ToolOutcome::OkJson(style_data_json(query, limit));
    }
    ToolOutcome::OkJson(design_data_json(query, platform.as_deref(), limit))
}
