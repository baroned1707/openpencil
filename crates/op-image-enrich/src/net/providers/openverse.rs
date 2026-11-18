//! Openverse transport: catalogue list, OAuth token, and the 429 / 401
//! recovery loop. Moved out of `catalog.rs` (pure code motion) when the
//! providers became [`super::ImageProvider`] variants with their own base URL.

use super::{
    catalog::{error_chain, read_json_capped},
    pacing::{
        cache_token, cached_token, drop_cached_token, record_response, retry_after_seconds,
        retry_delay, token_lock, wait_for_request,
    },
    WebOpenverseCredentials,
};

/// Catalogue/token calls get a longer budget than the shared 8 s client
/// timeout: Openverse answers a cold query in ~9 s on a slow day, and a
/// timed-out list request is indistinguishable from "no results" downstream.
const CATALOGUE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

struct TokenResult {
    token: String,
    from_cache: bool,
}

/// One Openverse list query as raw JSON. `None` = request-level failure
/// (429 / network / bad body); a catalogue answering zero hits is
/// `Some({"results": []})`, so the ladder can tell the two apart.
///
/// `license_type=commercial,modification` asks Openverse to return only works
/// usable commercially AND modifiable; the licence policy still re-checks
/// every hit because the provider's filter is a convenience, not a contract.
pub(crate) async fn fetch_list_json(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    aspect_ratio: Option<crate::ImageAspectRatio>,
    page_size: usize,
    credentials: Option<&WebOpenverseCredentials>,
) -> Option<serde_json::Value> {
    let mut url = reqwest::Url::parse_with_params(
        &format!("{base_url}/v1/images/"),
        &[
            ("q", query),
            ("page_size", &page_size.to_string()),
            ("license_type", "commercial,modification"),
        ],
    )
    .ok()?;
    if let Some(aspect_ratio) = aspect_ratio {
        url.query_pairs_mut()
            .append_pair("aspect_ratio", aspect_ratio.as_openverse_param());
    }
    fetch_openverse_json(client, base_url, url, query, credentials).await
}

pub(crate) async fn fetch_openverse_json(
    client: &reqwest::Client,
    base_url: &str,
    url: reqwest::Url,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> Option<serde_json::Value> {
    let mut token = None;
    let mut token_was_cached = false;
    if let Some(credentials) = credentials {
        if let Some(result) =
            fetch_openverse_token_with_source(client, base_url, credentials, false).await
        {
            token = Some(result.token);
            token_was_cached = result.from_cache;
        }
    }

    let mut retried_429 = false;
    let mut refreshed_401 = false;
    loop {
        if !wait_for_request(base_url).await {
            return None;
        }
        let mut request = client.get(url.clone()).timeout(CATALOGUE_REQUEST_TIMEOUT);
        if let Some(token) = token.as_deref() {
            request = request.bearer_auth(token);
        }
        let resp = match request.send().await {
            Ok(resp) => resp,
            Err(err) => {
                // A transport failure (today: Openverse cold queries taking
                // longer than the shared 8 s client timeout) used to vanish
                // as a silent `k=0`; name it so the log tells a slow API
                // apart from an empty catalogue.
                eprintln!(
                    "[ENRICH] openverse: request failed for \"{query}\": {}",
                    error_chain(&err)
                );
                return None;
            }
        };
        let status = resp.status();
        let retry_after = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            Some(retry_after_seconds(resp.headers()))
        } else {
            None
        };
        record_response(base_url, resp.headers());
        if !status.is_success() {
            if let Some(retry_after) = retry_after {
                eprintln!(
                    "[ENRICH] openverse: HTTP {status} for \"{query}\" (retry-after={retry_after}s)"
                );
            } else {
                eprintln!("[ENRICH] openverse: HTTP {status} for \"{query}\"");
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && !retried_429 {
                retried_429 = true;
                retry_delay(resp.headers()).await;
                continue;
            }
            if status == reqwest::StatusCode::UNAUTHORIZED && token_was_cached && !refreshed_401 {
                refreshed_401 = true;
                if let Some(credentials) = credentials {
                    drop_cached_token(&credentials.client_id);
                    if let Some(result) =
                        fetch_openverse_token_with_source(client, base_url, credentials, true).await
                    {
                        token = Some(result.token);
                        token_was_cached = result.from_cache;
                    } else {
                        token = None;
                        token_was_cached = false;
                    }
                    continue;
                }
            }
            return None;
        }
        return read_json_capped(resp).await;
    }
}

/// OAuth token for `credentials` against `base_url` (cached per client id).
pub(crate) async fn fetch_token(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &WebOpenverseCredentials,
) -> Option<String> {
    fetch_openverse_token_with_source(client, base_url, credentials, false)
        .await
        .map(|result| result.token)
}

pub async fn fetch_openverse_token(
    client: &reqwest::Client,
    credentials: &WebOpenverseCredentials,
) -> Option<String> {
    fetch_openverse_token_with_source(
        client,
        super::provider::OPENVERSE_BASE_URL,
        credentials,
        false,
    )
    .await
    .map(|result| result.token)
}

async fn fetch_openverse_token_with_source(
    client: &reqwest::Client,
    base_url: &str,
    credentials: &WebOpenverseCredentials,
    force_refresh: bool,
) -> Option<TokenResult> {
    let lock = token_lock(&credentials.client_id);
    let _guard = lock.lock().await;
    if !force_refresh {
        if let Some(token) = cached_token(&credentials.client_id) {
            return Some(TokenResult {
                token,
                from_cache: true,
            });
        }
    } else {
        drop_cached_token(&credentials.client_id);
    }

    let mut retried_429 = false;
    loop {
        if !wait_for_request(base_url).await {
            return None;
        }
        let resp = client
            .post(format!("{base_url}/v1/auth_tokens/token/"))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", credentials.client_id.as_str()),
                ("client_secret", credentials.client_secret.as_str()),
            ])
            .timeout(CATALOGUE_REQUEST_TIMEOUT)
            .send()
            .await;
        let resp = match resp {
            Ok(resp) => resp,
            Err(err) => {
                eprintln!(
                    "[ENRICH] openverse-token: request failed: {}",
                    error_chain(&err)
                );
                return None;
            }
        };
        let status = resp.status();
        let retry_after = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            Some(retry_after_seconds(resp.headers()))
        } else {
            None
        };
        record_response(base_url, resp.headers());
        if !status.is_success() {
            if let Some(retry_after) = retry_after {
                eprintln!("[ENRICH] openverse-token: HTTP {status} (retry-after={retry_after}s)");
            } else {
                eprintln!("[ENRICH] openverse-token: HTTP {status}");
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && !retried_429 {
                retried_429 = true;
                retry_delay(resp.headers()).await;
                continue;
            }
            return None;
        }
        let json = read_json_capped(resp).await?;
        let token = json
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string)?;
        if let Some(expires_in) = json.get("expires_in").and_then(serde_json::Value::as_u64) {
            cache_token(&credentials.client_id, token.clone(), expires_in);
        }
        return Some(TokenResult {
            token,
            from_cache: false,
        });
    }
}
