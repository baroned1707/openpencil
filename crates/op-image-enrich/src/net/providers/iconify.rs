//! Iconify transport and parser (keyless): `GET /search` for icon ids, then
//! `GET /<prefix>/<name>.svg` for the artwork.
//!
//! Every icon belongs to a collection with its own licence, which the search
//! answer carries; the licence policy (`iconify_spdx_policy`) decides per
//! collection. The SVG is embedded as `data:image/svg+xml;base64,…`: the
//! canvas, the export path and the popover all rasterize SVG data URLs at the
//! image-cache seam (`svg_raster::ensure_raster_bytes`), so no raster
//! conversion happens here. An SVG that could pull in anything else
//! (`<image>`, `<script>`, `<foreignObject>`, an external `href`, an entity
//! declaration) is refused, and the body is capped at 256 KiB.

use base64::Engine as _;

use super::catalog::{error_chain, read_json_capped, text_field, RawHit};
use super::pacing::{is_skipped, note_success, note_throttled};
use super::provider::SearchError;
use super::read_capped;

/// Iconify's documented minimum for `limit`.
const MIN_SEARCH_LIMIT: usize = 32;
const MAX_SEARCH_LIMIT: usize = 999;
/// Icon artwork is a few hundred bytes; 256 KiB bounds a misbehaving reply.
pub(crate) const MAX_ICON_SVG_BYTES: usize = 256 * 1024;
/// Pixel height requested for the icon (width follows its aspect), so the
/// rasterizer does not fall back to the SVG's `1em` default size.
const ICON_RENDER_HEIGHT: &str = "256";

pub(crate) fn search_url(base_url: &str, query: &str, limit: usize) -> Option<reqwest::Url> {
    let limit = limit.clamp(MIN_SEARCH_LIMIT, MAX_SEARCH_LIMIT).to_string();
    reqwest::Url::parse_with_params(
        &format!("{}/search", base_url.trim_end_matches('/')),
        &[("query", query), ("limit", limit.as_str())],
    )
    .ok()
}

/// One Iconify search as raw JSON. Repeated throttling skips the provider for
/// the rest of the process, like the other catalogues.
pub(crate) async fn fetch_search_json(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    limit: usize,
) -> Result<serde_json::Value, SearchError> {
    if is_skipped(base_url) {
        return Err(SearchError::Skipped);
    }
    let url = search_url(base_url, query, limit).ok_or(SearchError::RequestFailed)?;
    let resp = client.get(url).send().await.map_err(|err| {
        eprintln!(
            "[ENRICH] iconify: request failed for \"{query}\": {}",
            error_chain(&err)
        );
        SearchError::RequestFailed
    })?;
    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        eprintln!("[ENRICH] iconify: HTTP 429 for \"{query}\"");
        return Err(if note_throttled(base_url) {
            SearchError::Skipped
        } else {
            SearchError::RequestFailed
        });
    }
    if !status.is_success() {
        eprintln!("[ENRICH] iconify: HTTP {status} for \"{query}\"");
        return Err(SearchError::RequestFailed);
    }
    note_success(base_url);
    read_json_capped(resp)
        .await
        .filter(|json| json.get("icons").is_some_and(serde_json::Value::is_array))
        .ok_or(SearchError::RequestFailed)
}

/// Icon id parts are lowercase alphanumerics and dashes; anything else would
/// let a reply steer the artwork URL, so such ids are dropped.
fn valid_icon_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn parse_iconify_results(json: &serde_json::Value, base_url: &str) -> Vec<RawHit> {
    let Some(icons) = json.get("icons").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    let base_url = base_url.trim_end_matches('/');
    icons
        .iter()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|id| {
            let (prefix, name) = id.split_once(':')?;
            if !valid_icon_part(prefix) || !valid_icon_part(name) {
                return None;
            }
            let collection = json.get("collections").and_then(|all| all.get(prefix));
            let license = collection.and_then(|collection| collection.get("license"));
            let words = name.replace('-', " ");
            let collection_name = collection
                .and_then(|collection| text_field(collection, "name"))
                .unwrap_or_default();
            let creator = collection
                .and_then(|collection| collection.get("author"))
                .and_then(|author| text_field(author, "name"));
            let license_title = license.and_then(|license| text_field(license, "title"));
            Some(RawHit {
                id: id.to_string(),
                thumb_url: format!("{base_url}/{prefix}/{name}.svg?height={ICON_RENDER_HEIGHT}"),
                attribution: [creator.clone(), license_title.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(", "),
                relevance_metadata: format!("{words} {collection_name}"),
                title: words,
                provider: "iconify",
                creator,
                license: license_title,
                license_url: license.and_then(|license| text_field(license, "url")),
                license_id: license.and_then(|license| text_field(license, "spdx")),
                source_page_url: Some(format!("https://icon-sets.iconify.design/{prefix}/{name}/")),
                thumb_is_svg: true,
                ..RawHit::default()
            })
        })
        .collect()
}

/// Settings Test (and any health probe): the search answers with `icons`.
pub(crate) async fn probe(client: &reqwest::Client, base_url: &str) -> bool {
    fetch_search_json(client, base_url, "test", MIN_SEARCH_LIMIT)
        .await
        .is_ok()
}

/// An SVG document fit to embed: an `<svg>` root and none of the constructs
/// that pull in other content.
pub(crate) fn is_plain_icon_svg(svg: &str) -> bool {
    let lowered = svg.to_ascii_lowercase();
    lowered.contains("<svg")
        && ![
            "<script",
            "<foreignobject",
            "<image",
            "<!entity",
            "href=\"http",
            "href='http",
            "href=\"//",
            "href=\"file",
        ]
        .iter()
        .any(|forbidden| lowered.contains(forbidden))
}

/// Download one icon and embed it as an SVG data URL.
pub(crate) async fn fetch_svg_data_url(client: &reqwest::Client, url: &str) -> Option<String> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = read_capped(resp, MAX_ICON_SVG_BYTES).await?;
    let svg = String::from_utf8(bytes).ok()?;
    if !is_plain_icon_svg(&svg) {
        eprintln!("[ENRICH] iconify: refused an SVG with external or active content");
        return None;
    }
    Some(format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
    ))
}

#[cfg(test)]
#[path = "iconify_tests.rs"]
mod tests;
