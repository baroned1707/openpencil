//! Bounded upgrade from a selected catalogue thumbnail to its original image.

use std::time::{Duration, Instant};

use reqwest::Url;

#[cfg(test)]
use super::download::read_capped;
#[cfg(test)]
use std::net::SocketAddr;

use super::download::{normalize_image_mime_header, sniff_image_mime};
use super::ssrf_guard::{self, GuardPolicy};
use super::MAX_EMBEDDED_IMAGE_BYTES;

const ORIGINAL_FETCH_BUDGET: Duration = Duration::from_secs(8);
#[cfg(test)]
const MAX_REDIRECTS: usize = ssrf_guard::DEFAULT_MAX_REDIRECTS;
const MAX_ORIGINAL_PIXELS: u64 = 64_000_000;
const MAX_ORIGINAL_EDGE: u32 = 32_768;
const MAX_DELIVERED_EDGE: u32 = 2048;

/// Fetch only the selected hit's original. Failure leaves the caller's already
/// resolved thumbnail usable. DNS, redirects, body reads, and conversion share
/// one opportunity window; no redirect receives a fresh eight seconds.
pub(crate) async fn fetch_original_image_data_url(
    url: &str,
    deadline: Option<Instant>,
) -> Option<String> {
    let deadline = opportunity_deadline(Instant::now(), deadline)?;
    let url = safe_original_url(url)?;
    tokio::time::timeout_at(deadline.into(), fetch_until(url, deadline))
        .await
        .ok()
        .flatten()
}

fn opportunity_deadline(now: Instant, deadline: Option<Instant>) -> Option<Instant> {
    let deadline = deadline
        .unwrap_or(now + ORIGINAL_FETCH_BUDGET)
        .min(now + ORIGINAL_FETCH_BUDGET);
    (deadline > now).then_some(deadline)
}

async fn fetch_until(url: Url, deadline: Instant) -> Option<String> {
    let body = ssrf_guard::get_guarded(
        url.as_str(),
        &GuardPolicy::https_only(),
        deadline,
        MAX_EMBEDDED_IMAGE_BYTES,
        crate::HTTP_USER_AGENT,
    )
    .await
    .ok()?;
    let mime = body
        .content_type
        .as_deref()
        .and_then(normalize_image_mime_header);
    let bytes = body.bytes;
    let mime = mime.or_else(|| sniff_image_mime(&bytes).map(str::to_owned))?;
    // Decoding cannot block the async deadline or monopolize its worker.
    tokio::task::spawn_blocking(move || original_data_url(&mime, &bytes))
        .await
        .ok()
        .flatten()
}

fn original_data_url(mime: &str, bytes: &[u8]) -> Option<String> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;

    if bytes.len() > MAX_EMBEDDED_IMAGE_BYTES {
        return None;
    }
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if !safe_dimensions(width, height) {
        return None;
    }
    if width.max(height) > MAX_DELIVERED_EDGE {
        // Highly compressed originals may be smaller in bytes than their
        // resized version. Enforce geometry even when compression grows.
        let (mime, encoded) = crate::net::downscale::reencode_for_renderer(bytes)?;
        if encoded.len() > MAX_EMBEDDED_IMAGE_BYTES {
            return None;
        }
        return Some(format!("data:{mime};base64,{}", B64.encode(encoded)));
    }
    let url = crate::net::fetch::renderable_image_data_url(mime, bytes)?;
    let encoded = url.split_once(';')?.1.strip_prefix("base64,")?;
    // The shared converter can transcode WebP to PNG/JPEG. Bound the resulting
    // raw payload, rather than treating base64's 4/3 inflation as image bytes.
    let padding = encoded
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    let raw_len = encoded.len() / 4 * 3 - padding;
    (raw_len <= MAX_EMBEDDED_IMAGE_BYTES).then_some(url)
}

fn safe_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width.max(height) <= MAX_ORIGINAL_EDGE
        && u64::from(width) * u64::from(height) <= MAX_ORIGINAL_PIXELS
}

fn safe_original_url(value: &str) -> Option<Url> {
    ssrf_guard::guarded_url(value, &GuardPolicy::https_only())
}

#[cfg(test)]
fn checked_redirect(url: &Url, location: &str, followed: usize) -> Option<Url> {
    ssrf_guard::checked_redirect(url, location, followed, &GuardPolicy::https_only())
}

#[cfg(test)]
fn validate_addresses(addresses: Vec<SocketAddr>) -> Option<Vec<SocketAddr>> {
    ssrf_guard::validate_addresses(addresses, &GuardPolicy::https_only())
}

#[cfg(test)]
fn public_ip(ip: std::net::IpAddr) -> bool {
    ssrf_guard::public_ip(ip)
}

#[cfg(test)]
#[path = "original_download_tests.rs"]
mod tests;
