//! Characterization of the per-provider list stages the ladders are built
//! from. These pin the behaviours that differ between Openverse and
//! Wikimedia today (request-failure handling, stage labels) so the provider
//! refactor cannot silently unify them.

use super::*;
use crate::net::resolve::fenced_openverse_list_with;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const LONG_QUERY: &str = "minimal warm serum dropper bottle";
const RETRY_QUERY: &str = "serum dropper bottle";

fn dropper() -> RawHit {
    RawHit {
        id: "dropper".to_string(),
        thumb_url: "https://x/dropper.jpg".to_string(),
        title: "Serum dropper bottle".to_string(),
        relevance_metadata: "Serum dropper bottle product photo".to_string(),
        ..Default::default()
    }
}

#[test]
fn openverse_first_request_failure_has_no_retry_and_yields_no_list() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let list =
        crate::net::block_on_image_runtime(fenced_openverse_list_with(LONG_QUERY, "", move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            async { None }
        }));
    assert!(list.is_none(), "a transport failure is not an empty list");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "no retry after a failure");
}

#[test]
fn openverse_retry_request_failure_discards_the_whole_list() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let list =
        crate::net::block_on_image_runtime(fenced_openverse_list_with(LONG_QUERY, "", move |_| {
            let call = counted.fetch_add(1, Ordering::SeqCst);
            async move { (call == 0).then(Vec::new) }
        }));
    assert!(list.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn openverse_stage_labels_follow_the_request_that_produced_the_list() {
    let primary =
        crate::net::block_on_image_runtime(fenced_openverse_list_with(LONG_QUERY, "", |_| async {
            Some(vec![dropper()])
        }))
        .expect("list");
    assert_eq!(primary.provider, "openverse");
    assert_eq!(primary.provider_query, LONG_QUERY);

    let retried = crate::net::block_on_image_runtime(fenced_openverse_list_with(
        LONG_QUERY,
        "",
        |query| async move {
            Some(if query == LONG_QUERY {
                Vec::new()
            } else {
                vec![dropper()]
            })
        },
    ))
    .expect("list");
    assert_eq!(retried.provider, "openverse-retry");
    assert_eq!(retried.provider_query, RETRY_QUERY);
}

#[test]
fn wikimedia_failure_is_an_empty_list_and_still_sends_the_retry() {
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = requests.clone();
    let hits = crate::net::block_on_image_runtime(fetch_relevant_wikimedia_list_with(
        LONG_QUERY,
        move |query: String| {
            seen.lock().unwrap().push(query);
            async { Vec::new() }
        },
    ));
    assert!(hits.is_empty());
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [LONG_QUERY, RETRY_QUERY],
        "Wikimedia swallows transport errors, so the retry request is sent"
    );
}
