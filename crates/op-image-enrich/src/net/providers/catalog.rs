//! Provider catalogue parsing (Openverse and Wikimedia JSON → [`RawHit`])
//! plus thumbnail materialization. The requests themselves live in
//! `openverse.rs` / `wikimedia.rs`.

use super::{
    license::openverse_short_name, read_capped, WebImageSearchHit, MAX_EMBEDDED_IMAGE_BYTES,
    SEARCH_CANDIDATE_COUNT, SEARCH_RESULT_COUNT,
};

/// `reqwest::Error`'s `Display` stops at "error sending request"; the cause
/// (timeout, connection reset, TLS) sits in the source chain.
pub(crate) fn error_chain(err: &dyn std::error::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(inner) = source {
        parts.push(inner.to_string());
        source = inner.source();
    }
    parts.join(": ")
}

#[derive(Clone, Default)]
pub struct RawHit {
    pub(crate) id: String,
    pub(crate) thumb_url: String,
    /// Single display string (Openverse's own attribution line, or the
    /// Commons licence short name). Kept for the popover tooltip.
    pub(crate) attribution: String,
    pub(crate) title: String,
    pub(crate) relevance_metadata: String,
    pub(crate) original: Option<OriginalImageSource>,
    /// Stage id of the provider that returned the hit (`"openverse"`,
    /// `"wikimedia"`); empty only for hand-built fixtures.
    pub(crate) provider: &'static str,
    /// Plain-text creator (HTML stripped).
    pub(crate) creator: Option<String>,
    /// Short licence name, e.g. `"CC BY 4.0"`.
    pub(crate) license: Option<String>,
    pub(crate) license_url: Option<String>,
    /// Human landing page of the work at the provider.
    pub(crate) source_page_url: Option<String>,
}

impl RawHit {
    /// The creator / licence record to persist when this hit is chosen.
    /// `None` for hand-built hits that name no provider.
    pub(crate) fn provenance(&self) -> Option<crate::ImageProvenance> {
        (!self.provider.is_empty()).then(|| crate::ImageProvenance {
            provider: self.provider.to_string(),
            creator: self.creator.clone(),
            license: self.license.clone(),
            license_url: self.license_url.clone(),
            source_page_url: self.source_page_url.clone(),
        })
    }
}

/// The same catalogue artwork's original raster, never an invented URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalImageSource {
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

pub(crate) fn original_source(
    info: &serde_json::Value,
    thumbnail: &str,
) -> Option<OriginalImageSource> {
    let url = info.get("url")?.as_str()?.trim();
    if url.is_empty() || url == thumbnail {
        return None;
    }
    let dimension = |key| info.get(key)?.as_u64().and_then(|n| u32::try_from(n).ok());
    Some(OriginalImageSource {
        url: url.to_string(),
        width: dimension("width"),
        height: dimension("height"),
    })
}

/// Catalogue-list bodies are small JSON; 4 MiB bounds a misbehaving reply.
pub(super) async fn read_json_capped(resp: reqwest::Response) -> Option<serde_json::Value> {
    let bytes = read_capped(resp, MAX_EMBEDDED_IMAGE_BYTES).await?;
    serde_json::from_slice(&bytes).ok()
}

pub fn parse_openverse_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(results) = json.get("results").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            let thumb = r
                .get("thumbnail")
                .and_then(serde_json::Value::as_str)
                .or_else(|| r.get("url").and_then(serde_json::Value::as_str))?;
            let license_code = r
                .get("license")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let license_version = r
                .get("license_version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let license = format!("{license_code} {license_version}");
            Some(RawHit {
                id: r
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                thumb_url: thumb.to_string(),
                attribution: r
                    .get("attribution")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| license.trim().to_string()),
                title: r
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                relevance_metadata: openverse_relevance_metadata(r),
                original: original_source(r, thumb),
                provider: "openverse",
                creator: text_field(r, "creator"),
                // Openverse reports the code and the version separately; an
                // absent code stays `None` so the policy refuses the hit.
                license: openverse_short_name(license_code, license_version),
                license_url: text_field(r, "license_url"),
                source_page_url: text_field(r, "foreign_landing_url"),
            })
        })
        .take(SEARCH_CANDIDATE_COUNT)
        .collect()
}

fn openverse_relevance_metadata(result: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(title) = result.get("title").and_then(serde_json::Value::as_str) {
        parts.push(title.to_string());
    }
    if let Some(tags) = result.get("tags") {
        match tags {
            serde_json::Value::Array(items) => {
                for item in items {
                    if let Some(tag) = item
                        .as_str()
                        .or_else(|| item.get("name").and_then(serde_json::Value::as_str))
                    {
                        parts.push(tag.to_string());
                    }
                }
            }
            serde_json::Value::String(tags) => parts.push(tags.clone()),
            _ => {}
        }
    }
    parts.join(" ")
}

pub fn parse_wikimedia_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(pages) = json
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    pages
        .values()
        .filter_map(wikimedia_hit)
        .take(SEARCH_CANDIDATE_COUNT)
        .collect()
}

/// One Commons page as a [`RawHit`]; `None` for pages without image info, a
/// non-image MIME, or no usable URL. Shared by the list parser and the judged
/// ladder (which reads the same pages for their licence and credit).
pub(crate) fn wikimedia_hit(page: &serde_json::Value) -> Option<RawHit> {
    let info = page.get("imageinfo")?.as_array()?.first()?;
    if !wikimedia_info_is_image(page, info) {
        return None;
    }
    let thumb = info
        .get("thumburl")
        .and_then(serde_json::Value::as_str)
        .or_else(|| info.get("url").and_then(serde_json::Value::as_str))?;
    let meta = |key: &str| extmetadata_value(info, key);
    Some(RawHit {
        id: page
            .get("pageid")
            .map(|v| v.to_string())
            .unwrap_or_default(),
        thumb_url: thumb.to_string(),
        attribution: meta("LicenseShortName").unwrap_or_default(),
        title: page
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        relevance_metadata: page
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        original: original_source(info, thumb),
        provider: "wikimedia",
        // Commons `Artist` is an HTML fragment (links, spans).
        creator: meta("Artist")
            .map(|artist| strip_html(&artist))
            .filter(|artist| !artist.is_empty()),
        license: meta("LicenseShortName"),
        license_url: meta("LicenseUrl"),
        source_page_url: text_field(info, "descriptionurl"),
    })
}

/// Non-empty trimmed string field of a JSON object.
pub(crate) fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// `extmetadata.<key>.value` of a Commons `imageinfo` entry.
pub(crate) fn extmetadata_value(info: &serde_json::Value, key: &str) -> Option<String> {
    let value = info
        .get("extmetadata")?
        .get(key)?
        .get("value")?
        .as_str()?
        .trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Plain text of an HTML fragment: tags dropped, the common entities
/// decoded, whitespace collapsed. Commons `Artist` values are markup like
/// `<a href="//commons.wikimedia.org/wiki/User:X">Jane &amp; Co</a>`.
pub(crate) fn strip_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                // A tag boundary separates words ("A<br>B" is two words).
                text.push(' ');
            }
            _ if in_tag => {}
            _ => text.push(character),
        }
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Wikimedia can return page-one JPEG thumbnails for PDFs, audio, and video
/// files. Those thumbnails are renderable bytes but are not image-search
/// results, so accepting them turns an archival cover page into a product
/// photo. Trust the source MIME when it is present and retain a title-extension
/// fence for older/test payloads that omit `mime`.
pub(crate) fn wikimedia_info_is_image(page: &serde_json::Value, info: &serde_json::Value) -> bool {
    if let Some(mime) = info.get("mime").and_then(serde_json::Value::as_str) {
        return mime.trim().to_ascii_lowercase().starts_with("image/");
    }

    let title = page
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ![".pdf", ".ogg", ".oga", ".ogv", ".webm", ".mp3", ".mp4"]
        .iter()
        .any(|extension| title.ends_with(extension))
}

/// Download each hit's thumbnail into a `data:` URL through the caller's
/// materializer. Hits whose thumbnails fail to download are dropped.
pub(super) async fn materialize_thumbs<F, Fut>(
    hits: Vec<RawHit>,
    fetch_data_url: &F,
) -> Vec<WebImageSearchHit>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let mut out = Vec::with_capacity(hits.len().min(SEARCH_RESULT_COUNT));
    for hit in hits {
        if let Some(data_url) = fetch_data_url(hit.thumb_url.clone()).await {
            out.push(WebImageSearchHit {
                id: hit.id,
                thumb_data_url: data_url,
                attribution: hit.attribution,
            });
            if out.len() == SEARCH_RESULT_COUNT {
                break;
            }
        }
    }
    out
}

/// Materialize only the first usable thumbnail. Failed downloads advance to
/// the next provider hit, while the first success ends the loop immediately.
pub(super) async fn materialize_first_thumb<F, Fut>(
    hits: Vec<RawHit>,
    fetch_data_url: &F,
) -> Option<WebImageSearchHit>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    for hit in hits {
        if let Some(data_url) = fetch_data_url(hit.thumb_url).await {
            return Some(WebImageSearchHit {
                id: hit.id,
                thumb_data_url: data_url,
                attribution: hit.attribution,
            });
        }
    }
    None
}
