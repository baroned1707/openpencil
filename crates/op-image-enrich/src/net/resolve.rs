//! The unjudged single-image ladder (desktop session default, mobile FFI,
//! headless op-smoke fill) and the relevance-fenced Openverse list shared
//! with the judged ladder in `fetch.rs`.
//!
//! Until 2026-09-27 the unjudged ladder never ran the relevance fence: it
//! picked the Openverse result with the most raw title-token overlap and fell
//! back to the first result at zero overlap, retried with the query's FIRST
//! two words (dropping the head noun: "serum oil bottle" → "serum oil"), and
//! took Wikimedia's first page unseen. A skincare landing page shipped a
//! street-graffiti "snake oil serum" mural and an Instagram tattoo shot
//! ("… disposable tubes … cleanser #tattoo") as product photos that way.
//! Both lists now pass the same subject/photo/brand fence as the judged
//! ladder, with the photo contract read from the slot's image prompt too.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::net::fetch::{
    first_unused_renderable_image_src, settle_provider_identity, ImageCandidateClaim,
};
use crate::net::providers::{
    default_providers, fenced_list, run_ladder, simplify_search_query, FencedList, ImageProvider,
    LadderRequest, ProviderCtx, WebOpenverseCredentials,
};
use crate::net::search_trace::{short_title, trace_line};
use crate::{ImageAspectRatio, ImageKind};

/// The image a ladder settled on, with enough provenance for a one-line
/// diagnostic (`[IMAGE] <node> query=… -> <provider> "<title>"`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedImage {
    /// Renderable `data:` URL written into the slot.
    pub src: String,
    /// Ladder stage that produced it (`openverse`, `openverse-retry`,
    /// `openverse-rewrite`, `wikimedia`).
    pub provider: &'static str,
    /// The query actually sent to that provider.
    pub provider_query: String,
    /// Catalogue title of the chosen hit.
    pub title: String,
    /// Attribution and the original URL belong to this exact chosen artwork.
    pub attribution: String,
    pub original: Option<crate::net::providers::OriginalImageSource>,
    /// Who made the artwork and under which licence; `None` for hand-built
    /// values. Persisted on the node by `apply_result_with_credit`.
    pub provenance: Option<crate::ImageCredit>,
    /// Unsplash `download_location` of the chosen photo, pinged once when
    /// the image is applied. Transient: never written to the document.
    pub download_ping: Option<String>,
}

impl ResolvedImage {
    /// One-line, URL-free provenance: `openverse "Title" (via "q")`.
    pub fn describe(&self) -> String {
        format!(
            "{} \"{}\" (via \"{}\")",
            self.provider,
            short_title(&self.title, 80),
            short_title(&self.provider_query, 60)
        )
    }
}

pub fn fetch_first_image_resolved_blocking(
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<ResolvedImage> {
    fetch_first_image_resolved_blocking_with_deadline(
        query,
        intent,
        aspect_ratio,
        credentials,
        used_urls,
        None,
    )
}

pub fn fetch_first_image_resolved_blocking_with_deadline(
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
    deadline: Option<Instant>,
) -> Option<ResolvedImage> {
    fetch_first_image_resolved_blocking_with_providers(
        query,
        intent,
        aspect_ratio,
        credentials,
        used_urls,
        deadline,
        &default_providers(),
        ImageKind::Photo,
    )
}

/// The unjudged blocking ladder over the caller's provider list (see
/// `providers_for`) and slot kind.
#[allow(clippy::too_many_arguments)]
pub fn fetch_first_image_resolved_blocking_with_providers(
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
    deadline: Option<Instant>,
    providers: &[ImageProvider],
    kind: ImageKind,
) -> Option<ResolvedImage> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    // Shared runtime bridge: a private per-call runtime aborts with "Cannot
    // start a runtime from within a runtime" once this search is reached from
    // a tokio worker (design-loop / MCP driven runs).
    crate::net::block_on_image_runtime(async {
        let selected = thumbnail_before_deadline(
            deadline,
            fetch_first_image_resolved_thumbnail(
                query,
                intent,
                aspect_ratio,
                credentials,
                used_urls,
                providers,
                kind,
            ),
        )
        .await?;
        Some(super::original_delivery::upgrade_selected_image(selected, used_urls, deadline).await)
    })
}

async fn thumbnail_before_deadline<T>(
    deadline: Option<Instant>,
    future: impl std::future::Future<Output = Option<T>>,
) -> Option<T> {
    let Some(deadline) = deadline else {
        return future.await;
    };
    // Reserve the same final poll/save opportunity as the judged ladder.
    let deadline = deadline
        .checked_sub(Duration::from_millis(250))
        .unwrap_or(deadline);
    if Instant::now() >= deadline {
        eprintln!("[ENRICH] unjudged-search: deadline; no request started");
        return None;
    }
    match tokio::time::timeout_at(deadline.into(), future).await {
        Ok(result) => result,
        Err(_) => {
            eprintln!("[ENRICH] unjudged-search: deadline; no candidate completed");
            None
        }
    }
}

/// A cancelled download must not keep its provider identity reserved.
struct PendingProviderClaim<'a> {
    used_urls: &'a Mutex<HashSet<String>>,
    identity: String,
    settled: bool,
}

impl<'a> PendingProviderClaim<'a> {
    fn new(used_urls: &'a Mutex<HashSet<String>>, identity: String) -> Option<Self> {
        if !used_urls.lock().unwrap().insert(identity.clone()) {
            return None;
        }
        Some(Self {
            used_urls,
            identity,
            settled: false,
        })
    }

    fn settle(mut self, outcome: ImageCandidateClaim) -> Option<String> {
        self.settled = true;
        settle_provider_identity(self.used_urls, &self.identity, outcome)
    }
}

impl Drop for PendingProviderClaim<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.used_urls.lock().unwrap().remove(&self.identity);
        }
    }
}

/// Unjudged ladder: fenced Openverse (primary, then one concrete retry) →
/// fenced Wikimedia. The first fenced hit, in fence rank order, whose
/// thumbnail renders and is not already used by another slot wins. When
/// every list is fenced empty the slot stays unresolved (fallback tile)
/// rather than filling with an unrelated picture.
pub async fn fetch_first_image_resolved(
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<ResolvedImage> {
    let selected = fetch_first_image_resolved_thumbnail(
        query,
        intent,
        aspect_ratio,
        credentials,
        used_urls,
        &default_providers(),
        ImageKind::Photo,
    )
    .await?;
    Some(super::original_delivery::upgrade_selected_image(selected, used_urls, None).await)
}

async fn fetch_first_image_resolved_thumbnail(
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
    providers: &[ImageProvider],
    kind: ImageKind,
) -> Option<ResolvedImage> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(8))
        .user_agent(crate::HTTP_USER_AGENT)
        .build()
        .ok()?;
    resolve_thumbnail_over_kind(
        &client,
        providers,
        kind,
        query,
        intent,
        aspect_ratio,
        credentials,
        used_urls,
    )
    .await
}

/// The unjudged ladder over a caller-supplied client + provider list: the
/// shared [`run_ladder`] supplies each provider's fenced list (aspect relaxed
/// once, one concrete retry) and this consumer claims the first hit, in fence
/// rank order, whose thumbnail renders and is unused. When every list is
/// fenced empty the slot stays unresolved rather than taking an unrelated
/// picture.
#[cfg(test)]
pub(crate) async fn resolve_thumbnail_over(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<ResolvedImage> {
    resolve_thumbnail_over_kind(
        client,
        providers,
        ImageKind::Photo,
        query,
        intent,
        aspect_ratio,
        credentials,
        used_urls,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_thumbnail_over_kind(
    client: &reqwest::Client,
    providers: &[ImageProvider],
    kind: ImageKind,
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<ResolvedImage> {
    let query = simplify_search_query(query);
    trace_line(format_args!(
        "ladder=unjudged query=\"{query}\" intent=\"{}\" aspect={}",
        short_title(intent, 90),
        aspect_ratio.map_or("any", ImageAspectRatio::as_openverse_param)
    ));
    let ctx = ProviderCtx::new(client, credentials).with_kind(kind);
    let request = LadderRequest {
        query: &query,
        intent,
        aspect: aspect_ratio,
    };
    let resolved = run_ladder(providers, &ctx, &request, |provider, list| {
        let identity_prefix = format!("{}:", provider.id());
        async move { claim_first_fenced_hit(client, list, &identity_prefix, used_urls).await }
    })
    .await;
    if resolved.is_none() {
        trace_line(format_args!(
            "chosen: none for \"{query}\" (every provider list fenced empty or unusable)"
        ));
    }
    resolved
}

/// Walk a fenced list in rank order and claim the first hit whose provider
/// identity and rendered content are both unused.
async fn claim_first_fenced_hit(
    client: &reqwest::Client,
    list: FencedList,
    identity_prefix: &str,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<ResolvedImage> {
    for hit in list.hits {
        let identity = if hit.id.trim().is_empty() {
            format!("{identity_prefix}url:{}", hit.thumb_url)
        } else {
            format!("{identity_prefix}{}", hit.id)
        };
        let Some(claim) = PendingProviderClaim::new(used_urls, identity) else {
            trace_line(format_args!(
                "skip \"{}\": already used by another slot",
                short_title(&hit.title, 80)
            ));
            continue;
        };
        let outcome = if hit.thumb_is_svg {
            claim_svg_icon(client, &hit.thumb_url, used_urls).await
        } else {
            first_unused_renderable_image_src(client, vec![hit.thumb_url.clone()], used_urls).await
        };
        let Some(src) = claim.settle(outcome) else {
            trace_line(format_args!(
                "skip \"{}\": thumbnail unusable or duplicate content",
                short_title(&hit.title, 80)
            ));
            continue;
        };
        let provenance = hit.provenance();
        let resolved = ResolvedImage {
            src,
            provider: list.provider,
            provider_query: list.provider_query.clone(),
            title: hit.title,
            attribution: hit.attribution,
            original: hit.original,
            provenance,
            download_ping: hit.download_ping,
        };
        trace_line(format_args!(
            "chosen: {} — highest-ranked fenced hit whose thumbnail rendered",
            resolved.describe()
        ));
        return Some(resolved);
    }
    None
}

/// The SVG counterpart of [`first_unused_renderable_image_src`]: embed one
/// icon and claim its content so two slots do not take the same icon.
async fn claim_svg_icon(
    client: &reqwest::Client,
    url: &str,
    used_urls: &Mutex<HashSet<String>>,
) -> ImageCandidateClaim {
    match crate::net::providers::fetch_svg_data_url(client, url).await {
        None => ImageCandidateClaim::Unavailable,
        Some(src) if crate::net::fetch::claim_unused_image_src(used_urls, &src) => {
            ImageCandidateClaim::Claimed(src)
        }
        Some(_) => ImageCandidateClaim::Duplicate,
    }
}

/// Fetch and relevance-fence one Openverse list (primary query, then one
/// concrete retry). The fence always follows the authored `query` + `intent`,
/// so a shorter provider query never loosens the subject or photo contract.
/// The judged ladder's Openverse tiers call this.
pub(crate) async fn fetch_relevant_openverse_list_with_aspect(
    client: &reqwest::Client,
    query: &str,
    intent: &str,
    aspect_ratio: Option<ImageAspectRatio>,
    credentials: Option<&WebOpenverseCredentials>,
) -> Option<FencedList> {
    let ctx = ProviderCtx::new(client, credentials);
    let request = LadderRequest {
        query,
        intent,
        aspect: aspect_ratio,
    };
    fenced_list(&ImageProvider::openverse(), &ctx, &request, aspect_ratio).await
}

#[cfg(test)]
pub(crate) use crate::net::providers::fenced_openverse_list_with;

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
