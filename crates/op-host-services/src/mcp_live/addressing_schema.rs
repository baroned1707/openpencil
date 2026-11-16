//! The addressing arguments in the `tools/list` the live server advertises.
//!
//! Every live tool accepts an optional `documentId`, and every write tool an
//! optional `operationId`. Rather than hand-editing the ~140 static schemas
//! (which are also the in-process design agent's tool definitions, where
//! neither argument means anything), the live connection adds the two
//! properties to the finished `tools/list` response. That one post-processing
//! step covers the static catalog, the per-document kit insert tools and the
//! lean catalog alike. An endpoint bound to one document (`/mcp/d/<id>`) needs
//! no `documentId`, so its list is left as it is.

use serde_json::{json, Value};

use crate::mcp_serve::document_address::{
    DOCUMENT_ID_ARG, DOCUMENT_ID_HEADER_PARAM, OPERATION_ID_ARG,
};
use crate::mcp_serve::tool_profile::tool_writes;

/// The tool that exists to find the id; it never takes one.
const LIST_DOCUMENTS: &str = "list_documents";

fn document_id_property() -> Value {
    json!({
        "type": "string",
        "description": "Which open document this call is about, as returned by list_documents (the full d<N>.<nonce>; ids from an earlier session are refused). Optional when exactly one document is open or the endpoint is bound to one document; required otherwise.",
        // MCP 2026-07-28 streamable HTTP: a client mirrors this value into the
        // `Mcp-Param-DocumentId` header; the server checks it only when sent.
        "x-mcp-header": DOCUMENT_ID_HEADER_PARAM,
    })
}

fn operation_id_property() -> Value {
    json!({
        "type": "string",
        "description": "Optional idempotency key for this write. Retrying with the same operationId and the same arguments returns the first response (marked _meta.replayed) without applying again; reusing it with different arguments is refused. Remembered for 10 minutes.",
    })
}

/// Add `documentId` (every tool but `list_documents`) and `operationId`
/// (write tools) to each tool of a `tools/list` response. A response that is
/// not a tool list is returned unchanged. Neither property is ever required.
pub(super) fn annotate_tools_list(response: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(response) else {
        return response.to_string();
    };
    let Some(tools) = value
        .pointer_mut("/result/tools")
        .and_then(Value::as_array_mut)
    else {
        return response.to_string();
    };
    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str).map(str::to_string) else {
            continue;
        };
        if name == LIST_DOCUMENTS {
            continue;
        }
        let Some(schema) = tool.get_mut("inputSchema").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(properties) = schema
            .entry("properties")
            .or_insert_with(|| json!({}))
            .as_object_mut()
        else {
            continue;
        };
        properties.insert(DOCUMENT_ID_ARG.to_string(), document_id_property());
        if tool_writes(&name) {
            properties.insert(OPERATION_ID_ARG.to_string(), operation_id_property());
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[
        {"name":"get_document_info","description":"d","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
        {"name":"set_node_name","description":"d","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
        {"name":"list_documents","description":"d","inputSchema":{"type":"object","properties":{}}},
        {"name":"bare","description":"d","inputSchema":{"type":"object"}}]}}"#;

    #[test]
    fn document_id_goes_on_every_tool_and_operation_id_only_on_writes() {
        let annotated: Value = serde_json::from_str(&annotate_tools_list(LIST)).expect("json");
        let tools = annotated["result"]["tools"].as_array().expect("tools");
        let props = |index: usize| tools[index]["inputSchema"]["properties"].clone();
        assert!(props(0).get("documentId").is_some());
        assert!(props(0).get("operationId").is_none(), "reads ignore it");
        assert!(props(1).get("documentId").is_some());
        assert!(props(1).get("operationId").is_some());
        assert!(
            props(1).get("name").is_some(),
            "existing properties survive"
        );
        assert!(
            props(2).get("documentId").is_none(),
            "list_documents takes no id"
        );
        assert!(
            props(3).get("documentId").is_some(),
            "a schema without properties gets them"
        );
        // Never required, and the header annotation is on documentId only.
        assert_eq!(tools[1]["inputSchema"]["required"], json!(["name"]));
        assert_eq!(props(1)["documentId"]["x-mcp-header"], "DocumentId");
        assert!(props(1)["operationId"].get("x-mcp-header").is_none());
    }

    #[test]
    fn a_response_that_is_not_a_tool_list_is_unchanged() {
        let other = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[]}}"#;
        assert_eq!(annotate_tools_list(other), other);
        assert_eq!(annotate_tools_list("not json"), "not json");
    }
}
