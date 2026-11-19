//! Background search and generation workers, plus their single-flight publication.

use super::*;

// The parameter list mirrors the session state the spawned worker needs;
// grouping it into a context struct would move nine fields around for one
// call site, so the lint is allowed here.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_job(
    target: ImageSearchTarget,
    credentials: Option<OpenverseCredentials>,
    stock_keys: StockKeys,
    used_urls: Arc<Mutex<HashSet<String>>>,
    search_memo: Arc<Mutex<HashMap<SearchIntentKey, SearchMemoEntry>>>,
    request_id: u64,
    judge: Arc<dyn ImageRelevanceJudge>,
    judge_enabled: bool,
    intent: String,
    deadline: Option<Instant>,
) -> ImageSearchJob {
    #[cfg(test)]
    if let Some(job) = super::parked::stubbed_search_job(&target, &intent, deadline) {
        return job;
    }
    let (tx, rx) = mpsc::channel();
    let node_id = target.node_id.clone();
    let aspect_ratio = target.aspect_ratio;
    let stock_view = Some(target.judge_view);
    let key = search_intent_key_for_kind(
        &target.query,
        aspect_ratio,
        target.prompt.as_deref(),
        target.judge_view,
        target.kind,
    );
    // With no keys this is exactly [Openverse, Wikimedia]; a vector slot gets
    // Iconify first.
    let providers = providers_for(target.kind, &stock_keys);
    let intent = Some(intent);
    let judge_intent = target
        .prompt
        .as_deref()
        .filter(|prompt| !prompt.trim().is_empty())
        .unwrap_or(target.query.as_str())
        .to_string();
    // One full search intent, one in-flight request and one session result.
    // Rebuilt nodes subscribe to the same pending request instead of racing
    // duplicate searches; completed intents return from the memo.
    {
        let mut memo = search_memo.lock().unwrap();
        match memo.get_mut(&key) {
            Some(SearchMemoEntry::Ready(found)) => {
                let _ = tx.send(JobOutcome::Search(Some(found.clone())));
                return ImageSearchJob {
                    node_id,
                    deadline,
                    intent,
                    stock_view,
                    fallback_target: None,
                    rx,
                };
            }
            Some(SearchMemoEntry::Pending { waiters, .. }) => {
                waiters.push(tx);
                return ImageSearchJob {
                    node_id,
                    deadline,
                    intent,
                    stock_view,
                    fallback_target: None,
                    rx,
                };
            }
            None => {
                memo.insert(
                    key.clone(),
                    SearchMemoEntry::Pending {
                        request_id,
                        waiters: vec![tx],
                    },
                );
            }
        }
    }
    std::thread::spawn(move || {
        let found = if judge_enabled {
            fetch_first_image_resolved_blocking_with_judge_and_providers(
                &target.query,
                aspect_ratio,
                credentials.as_ref().map(OpenverseCredentials::as_web),
                &used_urls,
                judge.as_ref(),
                &judge_intent,
                deadline,
                target.judge_view,
                &providers,
                target.kind,
            )
            .map(found_image)
        } else {
            // No configured judge: the unjudged ladder, still fenced for
            // relevance. The slot's image prompt is passed as intent so a
            // product-photo slot also rejects artwork hits (a tattoo shot and
            // a graffiti mural once filled skincare product cards).
            op_image_enrich::net::resolve::fetch_first_image_resolved_blocking_with_providers(
                &target.query,
                &judge_intent,
                aspect_ratio,
                credentials.as_ref().map(OpenverseCredentials::as_web),
                &used_urls,
                deadline,
                &providers,
                target.kind,
            )
            .map(found_image)
        };
        publish_search_result(&search_memo, key, request_id, found);
    });
    ImageSearchJob {
        node_id,
        deadline,
        intent,
        stock_view,
        fallback_target: None,
        rx,
    }
}

fn found_image(resolved: op_image_enrich::net::resolve::ResolvedImage) -> FoundImage {
    FoundImage {
        url: resolved.src,
        credit: resolved.provenance,
        download_ping: resolved.download_ping,
    }
}

/// Publish only into the exact Pending entry that launched this request. A
/// reset may remove it and a new document may insert the same key before the
/// old thread returns; the request id keeps that old result isolated.
pub(super) fn publish_search_result(
    search_memo: &Arc<Mutex<HashMap<SearchIntentKey, SearchMemoEntry>>>,
    key: SearchIntentKey,
    request_id: u64,
    found: Option<FoundImage>,
) -> bool {
    let waiters = {
        let mut memo = search_memo.lock().unwrap();
        let matches_request = matches!(
            memo.get(&key),
            Some(SearchMemoEntry::Pending {
                request_id: pending_id,
                ..
            }) if *pending_id == request_id
        );
        if !matches_request {
            return false;
        }
        let Some(SearchMemoEntry::Pending { waiters, .. }) = memo.remove(&key) else {
            unreachable!("request identity was checked under the same lock")
        };
        if let Some(found) = found.as_ref() {
            memo.insert(key, SearchMemoEntry::Ready(found.clone()));
        }
        waiters
    };
    for waiter in waiters {
        let _ = waiter.send(JobOutcome::Search(found.clone()));
    }
    true
}

/// Enrich via the configured image-GEN model instead of stock search. Prefers the
/// node's bound `image_prompt`; falls back to the search keyword so a node that
/// only carries `image_search_query` still generates. Same `ImageSearchJob`
/// channel as search, so `poll_into` applies the resulting src identically.
pub(super) fn spawn_gen_job(
    target: ImageSearchTarget,
    profile: ImageGenProfile,
    deadline: Option<Instant>,
) -> ImageSearchJob {
    let (tx, rx) = mpsc::channel();
    let node_id = target.node_id.clone();
    let intent = Some(intent_fingerprint(&target, Some(&profile)));
    let prompt = op_image_enrich::compose_generation_prompt(&target);
    let width = target.width;
    let height = target.height;
    std::thread::spawn(move || {
        let result =
            crate::image_generate_host::run_generate_blocking(&prompt, &profile, width, height);
        let _ = tx.send(JobOutcome::Generate(result));
    });
    ImageSearchJob {
        node_id,
        deadline,
        intent,
        stock_view: None,
        // Kept so a degradable failure can re-run the slot as a search.
        fallback_target: Some(target),
        rx,
    }
}

pub(super) fn spawn_unavailable_gen_job(
    target: ImageSearchTarget,
    deadline: Option<Instant>,
) -> ImageSearchJob {
    let (tx, rx) = mpsc::channel();
    let node_id = target.node_id.clone();
    let intent = Some(intent_fingerprint(&target, None));
    // No profile is a CONFIG fault, not a runtime one: the failure stays
    // visible (placeholder) and must never degrade into a stock search.
    let _ = tx.send(JobOutcome::Search(None));
    ImageSearchJob {
        node_id,
        deadline,
        intent,
        stock_view: None,
        fallback_target: None,
        rx,
    }
}
