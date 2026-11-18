//! Outbound-request guard for URLs that come from documents or catalogues.
//!
//! A URL we did not choose (a provider's original-image link, a remote `src`
//! stored in a document) must never reach loopback, private, link-local or
//! cloud-metadata addresses, whatever name it hides behind. The guard
//! validates the URL, resolves its host ONCE, refuses unless every answer is
//! public, pins the client to those exact addresses (so a rebinding DNS
//! answer cannot swap in a private one between check and connect), follows
//! redirects by hand, and re-runs the whole check on every hop.
//!
//! Extracted from `original_download.rs` (which keeps its HTTPS-only policy
//! and behaviour) so `remote_image_host` shares one implementation.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::time::Instant;

use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::Url;

use super::download::read_capped;

pub(crate) const DEFAULT_MAX_REDIRECTS: usize = 3;

#[derive(Debug, Clone)]
pub struct GuardPolicy {
    /// Accept `http://` as well as `https://`.
    pub allow_http: bool,
    /// Redirect hops followed before the request is refused.
    pub max_redirects: usize,
    /// Exact addresses accepted even though they are not public. Empty in
    /// production; tests list the loopback address of their fake server.
    /// An allow-list rather than a bypass flag: everything else (private
    /// ranges, `169.254.169.254`, redirects to them) stays refused.
    pub extra_allowed_ips: Vec<IpAddr>,
}

impl GuardPolicy {
    pub fn https_only() -> Self {
        Self {
            allow_http: false,
            max_redirects: DEFAULT_MAX_REDIRECTS,
            extra_allowed_ips: Vec::new(),
        }
    }

    pub fn http_or_https() -> Self {
        Self {
            allow_http: true,
            ..Self::https_only()
        }
    }

    fn address_allowed(&self, ip: IpAddr) -> bool {
        public_ip(ip) || self.extra_allowed_ips.contains(&ip)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardError {
    /// The URL, its resolved addresses, or a redirect target is not allowed.
    Refused(&'static str),
    /// Transport failure (message from the HTTP client).
    Request(String),
    /// The final answer was not a success status.
    Status(u16),
    /// The body exceeded the cap.
    TooLarge,
    /// The overall deadline passed.
    DeadlineExceeded,
}

pub struct GuardedBody {
    /// Raw `Content-Type` header, if any.
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// GET `url` under `policy`, reading at most `cap` body bytes (the read
/// aborts as soon as the cap is crossed). DNS, redirects and the body read
/// share `deadline`.
pub async fn get_guarded(
    url: &str,
    policy: &GuardPolicy,
    deadline: Instant,
    cap: usize,
    user_agent: &str,
) -> Result<GuardedBody, GuardError> {
    let mut url = guarded_url(url, policy).ok_or(GuardError::Refused("destination not allowed"))?;
    for redirects in 0..=policy.max_redirects {
        let host = url
            .host_str()
            .ok_or(GuardError::Refused("url has no host"))?
            .trim_matches(['[', ']'])
            .to_string();
        let port = url
            .port_or_known_default()
            .ok_or(GuardError::Refused("url has no port"))?;
        let addresses = guarded_addresses(&host, port, policy)
            .await
            .ok_or(GuardError::Refused("host resolves to a non-public address"))?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(GuardError::DeadlineExceeded)?;
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(remaining)
            .user_agent(user_agent)
            // Pin validated DNS results for direct connections. System
            // proxies remain supported and own their remote DNS/connection
            // policy.
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|error| GuardError::Request(error.to_string()))?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| GuardError::Request(error.to_string()))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(GuardError::Refused("redirect without a location"))?;
            url = checked_redirect(&url, location, redirects, policy)
                .ok_or(GuardError::Refused("redirect target not allowed"))?;
            continue;
        }
        if !response.status().is_success() {
            return Err(GuardError::Status(response.status().as_u16()));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = read_capped(response, cap)
            .await
            .ok_or(GuardError::TooLarge)?;
        return Ok(GuardedBody {
            content_type,
            bytes,
        });
    }
    Err(GuardError::Refused("too many redirects"))
}

pub(crate) fn checked_redirect(
    url: &Url,
    location: &str,
    followed: usize,
    policy: &GuardPolicy,
) -> Option<Url> {
    if followed >= policy.max_redirects {
        return None;
    }
    guarded_url(url.join(location).ok()?.as_str(), policy)
}

/// Parse and vet a URL before any DNS lookup: scheme, no credentials, and a
/// host that is neither a non-public IP literal nor an obviously internal
/// name. `None` = refused.
pub(crate) fn guarded_url(value: &str, policy: &GuardPolicy) -> Option<Url> {
    let mut url = Url::parse(value).ok()?;
    let scheme_ok = url.scheme() == "https" || (policy.allow_http && url.scheme() == "http");
    if !scheme_ok || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?.trim_matches(['[', ']']);
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !policy.address_allowed(ip) {
            return None;
        }
    } else {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if !host.contains('.')
            || [
                "localhost",
                "local",
                "internal",
                "home",
                "lan",
                "test",
                "invalid",
                "example",
                "onion",
                "arpa",
            ]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
            || ["example.com", "example.net", "example.org"]
                .iter()
                .any(|name| host == *name || host.ends_with(&format!(".{name}")))
        {
            return None;
        }
    }
    url.set_fragment(None);
    Some(url)
}

pub(crate) async fn guarded_addresses(
    host: &str,
    port: u16,
    policy: &GuardPolicy,
) -> Option<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return policy
            .address_allowed(ip)
            .then(|| vec![SocketAddr::new(ip, port)]);
    }
    let host = host.to_string();
    let addresses = tokio::task::spawn_blocking(move || {
        (host.as_str(), port)
            .to_socket_addrs()
            .ok()
            .map(|addresses| addresses.collect::<Vec<_>>())
    })
    .await
    .ok()??;
    validate_addresses(addresses, policy)
}

pub(crate) fn validate_addresses(
    addresses: Vec<SocketAddr>,
    policy: &GuardPolicy,
) -> Option<Vec<SocketAddr>> {
    (!addresses.is_empty()
        && addresses
            .iter()
            .all(|address| policy.address_allowed(address.ip())))
    .then_some(addresses)
}

pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(matches!(a, 0 | 10 | 127 | 224..=255)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || (b == 0 && matches!(c, 0 | 2))))
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && matches!(b, 18 | 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Only global unicast; exclude protocol assignments, documentation,
            // and transition ranges that could tunnel to non-public IPv4.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] <= 0x01ff || segments[1] == 0x0db8))
                && segments[0] != 0x2002
                && !(segments[0] == 0x3fff && segments[1] <= 0x0fff)
        }
    }
}
