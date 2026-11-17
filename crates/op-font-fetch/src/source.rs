//! The only seam to the network. Everything in this crate asks a
//! [`FontSource`] for bytes; production plugs in an HTTPS client, tests plug
//! in [`crate::fake::FakeFontSource`].

use std::fmt;

/// One successful response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub bytes: Vec<u8>,
    /// Strong validator of the body when the server sent one; recorded in the
    /// catalog cache for diagnostics only.
    pub etag: Option<String>,
}

/// Why a request produced no body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// HTTP 404. The only status that means "not here, try the next place".
    NotFound,
    /// No connection, DNS failure or connect timeout.
    Offline,
    /// The request or the whole family ran out of time.
    Timeout,
    /// The body was larger than the cap passed to `get`.
    TooLarge,
    /// Any other HTTP status.
    Status(u16),
    /// Anything else (TLS, malformed response, ...).
    Other(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found (404)"),
            Self::Offline => f.write_str("offline"),
            Self::Timeout => f.write_str("timed out"),
            Self::TooLarge => f.write_str("response too large"),
            Self::Status(code) => write!(f, "unexpected HTTP status {code}"),
            Self::Other(detail) => write!(f, "request failed: {detail}"),
        }
    }
}

/// A blocking HTTPS GET with a body cap. Implementations MUST refuse a body
/// larger than `max_bytes` (preferably from `Content-Length`, before reading
/// it) and MUST map HTTP 404 to [`FetchError::NotFound`] and connectivity
/// failures to [`FetchError::Offline`] / [`FetchError::Timeout`].
pub trait FontSource: Send + Sync + 'static {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Response, FetchError>;
}
