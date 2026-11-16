//! The parsed shape of one HTTP request, as `read_http_request` returns it.

#[derive(Debug)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub host: Option<String>,
    pub origin: Option<String>,
    /// Legacy `X-OpenPencil-Token` header value, when present. The local and
    /// managed web-canvas daemons deliberately ignore it; managed lifecycle
    /// authentication, when used, stays in the `openpencil/shutdown` body.
    pub token: Option<String>,
    /// `Content-Type` header value, when present. Browser-facing JSON routes
    /// require `application/json` so cross-origin "simple requests" (which
    /// skip the CORS preflight) cannot reach them.
    pub content_type: Option<String>,
    /// `Authorization` header value, verbatim (scheme included), when
    /// present. The multi-account online daemon reads a `Bearer <token>`
    /// out of it; every other mode ignores it.
    pub authorization: Option<String>,
    /// `Cookie` header value, verbatim, when present. The online daemon
    /// extracts its session cookie from it; every other mode ignores it.
    pub cookie: Option<String>,
    /// The raw query string (no leading `?`), when the target had one.
    ///
    /// `path` keeps its query stripped so exact-path routing is unaffected;
    /// this is captured alongside so the online daemon can read the tenant
    /// parameter. `EventSource` cannot set headers, which is why the tenant
    /// travels in the query at all — see `share_routes::TENANT_QUERY`.
    pub query: Option<String>,
    /// `Mcp-Param-DocumentId` header value, verbatim, when present — the
    /// spec's mirror of the `documentId` argument (`x-mcp-header`).
    pub mcp_param_document_id: Option<String>,
}
