//! The real HTTPS [`FontSource`]: how the missing-font downloader reaches
//! `api.fontsource.org` and `raw.githubusercontent.com`.
//!
//! Same shape as `iconify_host` / `update_check`: rustls, a User-Agent, and the
//! shared runtime bridged through `block_on_anywhere` from a detached worker
//! thread. On top of that it is HTTPS-only and enforces the body cap while
//! streaming, so a hostile or mistaken response can never be buffered whole.

use std::time::Duration;

use op_font_fetch::{FetchError, FontSource, Response};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Per request, body included.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub(crate) struct HttpsFontSource {
    client: reqwest::Client,
}

impl HttpsFontSource {
    pub(crate) fn new() -> Result<Self, FetchError> {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("openpencil-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| FetchError::Other(format!("http client: {error}")))?;
        Ok(Self { client })
    }
}

fn map_error(error: reqwest::Error) -> FetchError {
    if error.is_timeout() {
        FetchError::Timeout
    } else if error.is_connect() {
        FetchError::Offline
    } else {
        FetchError::Other(error.to_string())
    }
}

impl FontSource for HttpsFontSource {
    fn get(&self, url: &str, max_bytes: usize) -> Result<Response, FetchError> {
        op_host_services::chat_runtime::block_on_anywhere(async {
            let mut response = self.client.get(url).send().await.map_err(map_error)?;
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(FetchError::NotFound);
            }
            if !status.is_success() {
                return Err(FetchError::Status(status.as_u16()));
            }
            // Refuse from the declared length before reading a single byte.
            if response
                .content_length()
                .is_some_and(|length| length > max_bytes as u64)
            {
                return Err(FetchError::TooLarge);
            }
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(map_error)? {
                if bytes.len() + chunk.len() > max_bytes {
                    return Err(FetchError::TooLarge);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(Response { bytes, etag })
        })
    }
}
