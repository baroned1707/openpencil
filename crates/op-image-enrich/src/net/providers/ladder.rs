//! The single provider ladder.
//!
//! [`run_ladder`] owns three decisions that used to be open-coded in each
//! ladder: the provider ORDER, the two-keyword RETRY (inside
//! [`fenced_list_with`]) and the FALLBACK from one provider to the next
//! (a provider that errors or yields nothing the caller accepts hands over to
//! the next). What a caller DOES with each provider's list (embed thumbnails,
//! claim an unused hit, ask a judge) stays in its `consume` closure.

use std::future::Future;

use crate::net::search_trace::{short_title, trace_hits, trace_line};
use crate::ImageAspectRatio;

use super::catalog::RawHit;
use super::provider::{ImageProvider, ProviderCtx, SearchError};
use super::{retain_relevant_hits_for_intent, two_keyword_retry};

/// A relevance-fenced provider list plus the stage/query that produced it.
pub(crate) struct FencedList {
    pub(crate) hits: Vec<RawHit>,
    pub(crate) provider: &'static str,
    pub(crate) provider_query: String,
}

/// What every ladder asks of the providers. `intent` is the slot's authored
/// image prompt (photo contract); `aspect` is a preference, relaxed once for
/// providers that support it.
pub(crate) struct LadderRequest<'a> {
    pub(crate) query: &'a str,
    pub(crate) intent: &'a str,
    pub(crate) aspect: Option<ImageAspectRatio>,
}

/// Walk `providers` in order. For each one, build its fenced list (primary
/// query, then at most one two-keyword retry) and hand it to `consume`; the
/// first `Some` ends the walk. A provider whose request fails is skipped
/// without a retry (no hammering), and the aspect preference is dropped once
/// before leaving a provider that supports it.
pub(crate) async fn run_ladder<T, F, Fut>(
    providers: &[ImageProvider],
    ctx: &ProviderCtx<'_>,
    request: &LadderRequest<'_>,
    mut consume: F,
) -> Option<T>
where
    F: FnMut(&ImageProvider, FencedList) -> Fut,
    Fut: Future<Output = Option<T>>,
{
    for provider in providers {
        // The aspect filter is a preference (the slot crops its fill), not
        // part of the subject contract: a square filter can answer zero hits
        // while the unfiltered catalogue has fenced product shots.
        let aspects: Vec<Option<ImageAspectRatio>> =
            if provider.supports_aspect() && request.aspect.is_some() {
                vec![request.aspect, None]
            } else {
                vec![if provider.supports_aspect() {
                    request.aspect
                } else {
                    None
                }]
            };
        for aspect in aspects {
            if aspect.is_none() && request.aspect.is_some() && provider.supports_aspect() {
                trace_line(format_args!(
                    "{}: aspect filter fenced empty, retrying without it",
                    provider.id()
                ));
            }
            let Some(list) = fenced_list(provider, ctx, request, aspect).await else {
                // Request-level failure (429 / network): don't hammer the API.
                break;
            };
            if let Some(found) = consume(provider, list).await {
                return Some(found);
            }
        }
    }
    None
}

/// One provider's fenced list for `request` at `aspect`.
pub(crate) async fn fenced_list(
    provider: &ImageProvider,
    ctx: &ProviderCtx<'_>,
    request: &LadderRequest<'_>,
    aspect: Option<ImageAspectRatio>,
) -> Option<FencedList> {
    // The photo contract in `intent` ("product photo", brand fences) does not
    // apply to icons and vector art, so a vector request fences on the query
    // alone.
    let intent = match ctx.kind {
        crate::ImageKind::Photo => request.intent,
        crate::ImageKind::Vector => "",
    };
    fenced_list_with_policy(
        provider.id(),
        provider.retry_stage(),
        provider.fenced_by_relevance(),
        request.query,
        intent,
        |candidate| async move { provider.search(ctx, &candidate, aspect).await },
    )
    .await
}

/// Transport-free core: `fetch_list` answers one provider query (`Err` =
/// request failure). Relevance-fences the primary answer; when the answer is
/// empty or fully fenced, retries ONCE with the concrete tail of the query
/// ([`two_keyword_retry`]) — still fenced against the authored query + intent
/// so a shorter provider query never loosens the subject or photo contract.
/// A failed request, primary or retry, yields `None` (the whole list is
/// discarded, no further requests).
#[cfg(test)]
pub(crate) async fn fenced_list_with<F, Fut>(
    provider_id: &'static str,
    retry_stage: Option<&'static str>,
    query: &str,
    intent: &str,
    fetch_list: F,
) -> Option<FencedList>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<RawHit>, SearchError>>,
{
    fenced_list_with_policy(provider_id, retry_stage, true, query, intent, fetch_list).await
}

/// [`fenced_list_with`] with the lexical fence switchable. A provider that
/// ranks by icon NAME (Iconify) answers `fence = false`: its hits are the
/// provider's own matches, kept as returned, with no two-keyword retry (an
/// icon called `cart` has one word of evidence for "shopping cart", which
/// the two-token subject rule would drop).
pub(crate) async fn fenced_list_with_policy<F, Fut>(
    provider_id: &'static str,
    retry_stage: Option<&'static str>,
    fence: bool,
    query: &str,
    intent: &str,
    mut fetch_list: F,
) -> Option<FencedList>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<RawHit>, SearchError>>,
{
    let hits = fetch_list(query.to_string()).await.ok()?;
    trace_hits(provider_id, query, (query, intent), &hits);
    if !fence {
        return Some(FencedList {
            hits,
            provider: provider_id,
            provider_query: query.to_string(),
        });
    }
    let relevant = retain_relevant_hits_for_intent(hits, query, intent);
    if !relevant.is_empty() {
        return Some(FencedList {
            hits: relevant,
            provider: provider_id,
            provider_query: query.to_string(),
        });
    }

    let Some(retry_query) = two_keyword_retry(query) else {
        return Some(FencedList {
            hits: relevant,
            provider: provider_id,
            provider_query: query.to_string(),
        });
    };
    let retry = fetch_list(retry_query.clone()).await.ok()?;
    trace_hits(
        &format!("{provider_id}-retry"),
        &retry_query,
        (query, intent),
        &retry,
    );
    let retry_total = retry.len();
    let relevant = retain_relevant_hits_for_intent(retry, query, intent);
    if relevant.is_empty() && retry_total > 0 {
        eprintln!(
            "[ENRICH] {provider_id}: \"{}\" {retry_total} hits for \"{}\", none lexically relevant",
            short_title(query, 80),
            short_title(&retry_query, 80)
        );
    }
    // Wikimedia keeps reporting its primary stage and query for a retried
    // list (its historical label); Openverse labels the retry.
    let (provider, provider_query) = match retry_stage {
        Some(stage) => (stage, retry_query),
        None => (provider_id, query.to_string()),
    };
    Some(FencedList {
        hits: relevant,
        provider,
        provider_query,
    })
}

/// Test adapter: the Openverse list stage over an `Option`-returning fake.
#[cfg(test)]
pub(crate) async fn fenced_openverse_list_with<F, Fut>(
    query: &str,
    intent: &str,
    mut fetch_list: F,
) -> Option<FencedList>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<Vec<RawHit>>>,
{
    fenced_list_with(
        "openverse",
        Some("openverse-retry"),
        query,
        intent,
        |candidate| {
            let answer = fetch_list(candidate);
            async move { answer.await.ok_or(SearchError::RequestFailed) }
        },
    )
    .await
}

/// Test adapter: the unjudged popover/MCP Openverse list (empty intent).
#[cfg(test)]
pub(crate) async fn fetch_relevant_openverse_list_with<F, Fut>(
    query: &str,
    fetch_list: F,
) -> Option<Vec<RawHit>>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<Vec<RawHit>>>,
{
    fenced_openverse_list_with(query, "", fetch_list)
        .await
        .map(|list| list.hits)
}

/// Test adapter: the Wikimedia list stage over an infallible fake.
#[cfg(test)]
pub(crate) async fn fetch_relevant_wikimedia_list_with<F, Fut>(
    query: &str,
    mut fetch_list: F,
) -> Vec<RawHit>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Vec<RawHit>>,
{
    fenced_list_with("wikimedia", None, query, "", |candidate| {
        let answer = fetch_list(candidate);
        async move { Ok(answer.await) }
    })
    .await
    .map(|list| list.hits)
    .unwrap_or_default()
}
