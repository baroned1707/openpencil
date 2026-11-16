//! How large a request body each endpoint accepts.
//!
//! The HTTP reader used to apply one ceiling (64 MiB, sized for a whole
//! document with embedded images) to everything, so a local process could make
//! the live editor buffer 64 MiB for a `tools/call` that never needs more than
//! a few. [`BodyLimits::LIVE`] gives the live server a tight cap on JSON-RPC
//! calls, keeps the document-sync ceiling, and answers `413 Payload Too Large`
//! BEFORE reading a body byte (the refusal looks only at the declared
//! `Content-Length`). Every other reader keeps [`BodyLimits::STANDARD`], the
//! single historical ceiling and its historical error.

use super::document_address::parse_route;
use super::{is_document_sync_route, McpServeError};

/// The ceiling for whole-document sync and every route not named below.
pub const DOCUMENT_BODY_MAX: usize = 64 * 1024 * 1024;

/// The live server's cap on a JSON-RPC call. A `tools/call` carries a script, a
/// node list or an inline SVG / HTML; anything larger belongs in a file passed
/// by path (`svgPath`, `htmlPath`, `snapshotPath`).
pub const LIVE_TOOL_CALL_BODY_MAX: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyLimits {
    json_rpc: usize,
    refuse_with_413: bool,
}

impl BodyLimits {
    /// One 64 MiB ceiling for every route, refused as a protocol error.
    pub const STANDARD: Self = Self {
        json_rpc: DOCUMENT_BODY_MAX,
        refuse_with_413: false,
    };

    /// The live editor: [`LIVE_TOOL_CALL_BODY_MAX`] for JSON-RPC calls,
    /// [`DOCUMENT_BODY_MAX`] for document sync, both refused with `413`.
    pub const LIVE: Self = Self {
        json_rpc: LIVE_TOOL_CALL_BODY_MAX,
        refuse_with_413: true,
    };

    /// The cap for a request, in bytes.
    fn cap_for(&self, method: &str, path: &str) -> usize {
        if !is_document_sync_route(method, path) && method == "POST" && parse_route(path).is_some()
        {
            self.json_rpc
        } else {
            DOCUMENT_BODY_MAX
        }
    }

    /// The refusal for a request that declares `content_length` bytes, if it
    /// is over its cap.
    pub fn refusal(
        &self,
        method: &str,
        path: &str,
        content_length: usize,
    ) -> Option<McpServeError> {
        let cap = self.cap_for(method, path);
        if content_length <= cap {
            return None;
        }
        let message = format!("request body exceeds {} MiB", cap / (1024 * 1024));
        Some(if self.refuse_with_413 {
            McpServeError::Framing {
                status: "413 Payload Too Large",
                message,
            }
        } else {
            McpServeError::Protocol(message)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(error: Option<McpServeError>) -> Option<&'static str> {
        match error {
            Some(McpServeError::Framing { status, .. }) => Some(status),
            _ => None,
        }
    }

    #[test]
    fn live_caps_tool_calls_tightly_and_document_sync_loosely() {
        let live = BodyLimits::LIVE;
        let over_call = LIVE_TOOL_CALL_BODY_MAX + 1;
        assert_eq!(
            status(live.refusal("POST", "/mcp", over_call)),
            Some("413 Payload Too Large")
        );
        assert_eq!(
            status(live.refusal("POST", "/mcp/d/d3.a1b2c3", over_call)),
            Some("413 Payload Too Large")
        );
        assert!(live
            .refusal("POST", "/mcp", LIVE_TOOL_CALL_BODY_MAX)
            .is_none());
        // A document is far bigger than a call and is still welcome.
        assert!(live
            .refusal("POST", "/api/mcp/document", over_call)
            .is_none());
        assert_eq!(
            status(live.refusal("POST", "/api/mcp/document", DOCUMENT_BODY_MAX + 1)),
            Some("413 Payload Too Large")
        );
    }

    #[test]
    fn standard_readers_keep_the_single_historical_ceiling() {
        let standard = BodyLimits::STANDARD;
        assert!(standard
            .refusal("POST", "/mcp", DOCUMENT_BODY_MAX)
            .is_none());
        assert!(matches!(
            standard.refusal("POST", "/mcp", DOCUMENT_BODY_MAX + 1),
            Some(McpServeError::Protocol(_))
        ));
    }
}
