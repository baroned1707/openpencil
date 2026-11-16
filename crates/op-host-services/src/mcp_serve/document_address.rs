//! How an MCP tool call names the document it is about.
//!
//! The editor keeps several documents open, and the MCP spec (2026-07-28)
//! removed protocol-level sessions in favour of "server-minted handles passed
//! as ordinary tool arguments". So a call addresses a document with an
//! explicit `documentId` argument (`d<N>`, the editor's stable
//! `DocumentId(N)`), never with a session and never with "whatever is open".
//!
//! This module is the pure half: id text, splitting the addressing arguments
//! off a request body, the `/mcp/d/<N>` route, the `Mcp-Param-DocumentId`
//! header check, and the headless (single document, `d1`) normalisation. The
//! live server's resolution and locking live in `mcp_live`.

use std::borrow::Cow;

use base64::Engine as _;
use op_editor_core::DocumentId;

use super::document_nonce::launch_nonce;
pub use super::document_nonce::LaunchNonce;
use super::tool_catalog::McpToolCatalog;

/// Tool argument naming the target document. Optional on every call.
pub const DOCUMENT_ID_ARG: &str = "documentId";
/// Tool argument naming a write call for idempotent retries. Optional.
pub const OPERATION_ID_ARG: &str = "operationId";
/// The `x-mcp-header` annotation on `documentId`: the spec mirrors the value
/// into the `Mcp-Param-DocumentId` request header.
pub const DOCUMENT_ID_HEADER_PARAM: &str = "DocumentId";
/// The mirrored header, lower-cased (header names are case-insensitive).
pub const DOCUMENT_ID_HEADER: &str = "mcp-param-documentid";
/// JSON-RPC error code the spec assigns to `HeaderMismatch`.
pub const HEADER_MISMATCH_CODE: i64 = -32020;
/// The one document every headless mode serves.
pub const HEADLESS_DOCUMENT: DocumentId = DocumentId(1);

/// `d<N>` for a document id: the headless form, which has no earlier launch to
/// be confused with.
pub fn format_document_id(id: DocumentId) -> String {
    format!("d{}", id.0)
}

/// `d<N>` for a headless document, `d<N>.<nonce>` for a live one.
pub fn format_spoken_id(id: DocumentId, nonce: Option<LaunchNonce>) -> String {
    match nonce {
        Some(nonce) => format!("d{}.{nonce}", id.0),
        None => format_document_id(id),
    }
}

/// The id named by `raw`, or `None` when `raw` is not the canonical `d<N>`
/// spelling (no sign, no leading zeros), so one document has exactly one name.
pub fn parse_document_id(raw: &str) -> Option<DocumentId> {
    let digits = raw.strip_prefix('d')?;
    let number: u64 = digits.parse().ok()?;
    (number.to_string() == digits).then_some(DocumentId(number))
}

/// A document as an agent wrote it: the bare `d<N>` of the old spelling, or
/// the live `d<N>.<nonce>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentRef {
    pub id: DocumentId,
    pub nonce: Option<LaunchNonce>,
}

impl DocumentRef {
    /// Whether two spellings can name the same document: same number, and the
    /// same nonce whenever both carry one.
    pub fn agrees_with(&self, other: &DocumentRef) -> bool {
        self.id == other.id
            && match (self.nonce, other.nonce) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
    }
}

/// The reference spelled by `d<N>` or `d<N>.<nonce>` (canonical digits, six
/// lowercase hex nonce), or `None` for anything else.
pub fn parse_document_ref(raw: &str) -> Option<DocumentRef> {
    parse_ref_body(raw.strip_prefix('d')?)
}

fn parse_ref_body(body: &str) -> Option<DocumentRef> {
    let (digits, nonce) = match body.split_once('.') {
        Some((digits, nonce)) => (digits, Some(LaunchNonce::parse(nonce)?)),
        None => (body, None),
    };
    let number: u64 = digits.parse().ok()?;
    (number.to_string() == digits).then_some(DocumentRef {
        id: DocumentId(number),
        nonce,
    })
}

/// The addressing arguments of one call, taken off the request body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallAddress {
    /// The `documentId` argument as the client wrote it.
    pub document_id: Option<String>,
    /// The `operationId` argument as the client wrote it.
    pub operation_id: Option<String>,
}

/// Remove `documentId` / `operationId` from the call's arguments and return
/// them, so no tool implementation ever sees either. The body comes back
/// untouched (borrowed) when it names neither, which is the common case and
/// keeps multi-megabyte `batch_design` bodies from being re-serialized.
///
/// Both the real `tools/call` envelope (`params.arguments`) and the legacy
/// direct shape (`params`) are handled, exactly the two shapes
/// `op_mcp::parse_tool_call` accepts. Empty values count as absent.
pub fn split_address(body: &str) -> (CallAddress, Cow<'_, str>) {
    let untouched = || (CallAddress::default(), Cow::Borrowed(body));
    if !body.contains("\"documentId\"") && !body.contains("\"operationId\"") {
        return untouched();
    }
    let Ok(mut root) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
        return untouched();
    };
    let is_tools_call = root.get("method").and_then(|m| m.as_str()) == Some("tools/call");
    let arguments = if is_tools_call {
        root.get_mut("params").and_then(|p| p.get_mut("arguments"))
    } else {
        root.get_mut("params")
    };
    let Some(arguments) = arguments.and_then(|a| a.as_object_mut()) else {
        return untouched();
    };
    let removed_document = arguments.remove(DOCUMENT_ID_ARG);
    let removed_operation = arguments.remove(OPERATION_ID_ARG);
    if removed_document.is_none() && removed_operation.is_none() {
        return untouched();
    }
    let address = CallAddress {
        document_id: removed_document.and_then(argument_text),
        operation_id: removed_operation.and_then(argument_text),
    };
    (address, Cow::Owned(root.to_string()))
}

/// The text of an addressing argument: strings as written, other scalars in
/// their JSON spelling (so a numeric `documentId` is reported back as the
/// wrong value it is), `null` and blank as absent.
fn argument_text(value: serde_json::Value) -> Option<String> {
    let text = match value {
        serde_json::Value::Null => return None,
        serde_json::Value::String(text) => text,
        other => other.to_string(),
    };
    (!text.trim().is_empty()).then_some(text)
}

/// A live endpoint path, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveRoute {
    pub catalog: McpToolCatalog,
    /// The document the route itself names (`/mcp/d/<id>`), if any.
    pub document: Option<DocumentRef>,
}

/// Decode a request path: `/mcp` and `/` (full), `/mcp/lean`, and the
/// document-bound `/mcp/d/<id>` and `/mcp/d/<id>/lean`, where `<id>` is
/// `d<N>.<nonce>` (or the bare `d<N>`; the `d` is tolerated for a client that
/// builds the path from the number). Whether the nonce is this launch's is the
/// resolver's call, not the parser's. Anything else is `None` (404).
pub fn parse_route(path: &str) -> Option<LiveRoute> {
    if let Some(catalog) = McpToolCatalog::for_http_path(path) {
        return Some(LiveRoute {
            catalog,
            document: None,
        });
    }
    let rest = path.strip_prefix("/mcp/d/")?;
    let (segment, catalog) = match rest.split_once('/') {
        None => (rest, McpToolCatalog::Full),
        Some((segment, "lean")) => (segment, McpToolCatalog::Lean),
        Some(_) => return None,
    };
    let document = parse_ref_body(segment.strip_prefix('d').unwrap_or(segment))?;
    Some(LiveRoute {
        catalog,
        document: Some(document),
    })
}

/// The URL an agent spawned for `document` should use: bound to that document
/// so the agent never has to name it (`/mcp/d/d3.a1b2c3`, or with `/lean`). It
/// carries this launch's nonce, so it stops working when the editor restarts
/// instead of silently reaching another document.
pub fn mcp_url_for_document(port: u16, document: DocumentId, lean: bool) -> String {
    let suffix = if lean { "/lean" } else { "" };
    let id = format_spoken_id(document, Some(launch_nonce()));
    format!("http://127.0.0.1:{port}/mcp/d/{id}{suffix}")
}

/// Why a `Mcp-Param-DocumentId` header was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderMismatch {
    /// The header holds characters a header may not carry, or a malformed
    /// `=?base64?…?=` value.
    Malformed,
    /// The header (decoded) differs from the body's `documentId`, or the body
    /// has none.
    Differs {
        header: String,
        body: Option<String>,
    },
}

impl std::fmt::Display for HeaderMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => {
                f.write_str("HeaderMismatch: Mcp-Param-DocumentId is not a valid header value")
            }
            Self::Differs { header, body: Some(body) } => write!(
                f,
                "HeaderMismatch: Mcp-Param-DocumentId is '{header}' but the body's documentId is '{body}'"
            ),
            Self::Differs { header, body: None } => write!(
                f,
                "HeaderMismatch: Mcp-Param-DocumentId is '{header}' but the body has no documentId"
            ),
        }
    }
}

/// Check the optional `Mcp-Param-DocumentId` header against the body's
/// `documentId`. A client that sends no header is never refused (older
/// protocol versions do not send it); one that sends it must agree with the
/// body, because an intermediary may have routed on the header.
pub fn check_document_header(
    header: Option<&str>,
    body_value: Option<&str>,
) -> Result<(), HeaderMismatch> {
    let Some(header) = header else {
        return Ok(());
    };
    let decoded = decode_header_value(header).ok_or(HeaderMismatch::Malformed)?;
    if body_value == Some(decoded.as_str()) {
        return Ok(());
    }
    Err(HeaderMismatch::Differs {
        header: decoded,
        body: body_value.map(str::to_string),
    })
}

/// Plain header values must be visible ASCII, space or tab; anything else is
/// carried as `=?base64?<standard base64 of the UTF-8>?=`.
fn decode_header_value(raw: &str) -> Option<String> {
    if let Some(encoded) = raw
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?;
        return String::from_utf8(bytes).ok();
    }
    raw.bytes()
        .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
        .then(|| raw.to_string())
}

/// The JSON-RPC error body for a refused header (answered with HTTP 400).
pub fn header_mismatch_body(request_body: &str, mismatch: &HeaderMismatch) -> String {
    let id = super::sniff::sniff_id_raw(request_body.trim()).unwrap_or_else(|| "null".to_string());
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": serde_json::from_str::<serde_json::Value>(&id).unwrap_or(serde_json::Value::Null),
        "error": {"code": HEADER_MISMATCH_CODE, "message": mismatch.to_string()},
    })
    .to_string()
}

/// A `tools/call` error envelope (`isError:true`) answering `line`, or `None`
/// when no request id can be recovered from it.
pub fn tool_error_response(line: &str, message: String) -> Option<String> {
    let id = op_mcp::parse_tool_call(line.trim())
        .map(|call| call.id)
        .or_else(|| op_mcp::parser::extract_request_id(line.trim()))?;
    Some(op_mcp::tool_response_to_json(&op_mcp::ToolResponse::Err {
        id,
        code: op_mcp::ToolErrorCode::ToolFailed,
        message,
    }))
}

/// What a headless mode does with a tool-call body.
#[derive(Debug, PartialEq, Eq)]
pub enum HeadlessCall<'a> {
    /// Dispatch this body (the addressing arguments already removed).
    Dispatch(Cow<'a, str>),
    /// Answer with this refusal instead; the call names another document.
    Refuse(String),
}

/// Headless modes serve exactly one document, `d1`. Strip the addressing
/// arguments and refuse a `documentId` naming anything else; a client that
/// names none (every client that predates addressing) is unaffected.
pub fn headless_line(line: &str) -> HeadlessCall<'_> {
    let (address, stripped) = split_address(line);
    match address.document_id.as_deref() {
        Some(named) if parse_document_id(named) != Some(HEADLESS_DOCUMENT) => {
            let message = format!(
                "unknown document '{named}': this server has exactly one document, {}",
                format_document_id(HEADLESS_DOCUMENT)
            );
            HeadlessCall::Refuse(tool_error_response(&stripped, message).unwrap_or_default())
        }
        _ => HeadlessCall::Dispatch(stripped),
    }
}

#[cfg(test)]
#[path = "document_address_tests.rs"]
mod tests;
