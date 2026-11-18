//! The judged Wikimedia path reads Commons pages directly (not through
//! `RawHit` lists), so it needs its own proof that a refused licence is
//! dropped before any thumbnail download or judge call.

use super::*;
use crate::net::test_support::{commons_page, commons_reply, test_client, FakeHttp, FakeResponse};
use crate::net::{JudgedCandidate, RelevanceVerdict};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct CountingJudge {
    thumbs_seen: AtomicUsize,
}

impl ImageRelevanceJudge for CountingJudge {
    fn judge(&self, _query: &str, _intent: &str, thumbs: &[Vec<u8>]) -> Vec<JudgedCandidate> {
        self.thumbs_seen.fetch_add(thumbs.len(), Ordering::SeqCst);
        (0..thumbs.len())
            .map(|index| JudgedCandidate {
                index,
                verdict: RelevanceVerdict::On,
                reason: String::new(),
            })
            .collect()
    }
}

fn commons_with(pages: Vec<(&'static str, &'static str)>) -> FakeHttp {
    let base = Arc::new(std::sync::Mutex::new(String::new()));
    let handler_base = base.clone();
    let server = FakeHttp::start(move |target, _| {
        if target.starts_with("/thumb/") {
            return FakeResponse::png();
        }
        let base = handler_base.lock().unwrap().clone();
        commons_reply(
            pages
                .iter()
                .enumerate()
                .map(|(index, (short, url))| {
                    commons_page(
                        &base,
                        10 + index as u64,
                        "File:Serum dropper.png",
                        short,
                        url,
                    )
                })
                .collect(),
        )
    });
    *base.lock().unwrap() = server.base_url.clone();
    server
}

fn judged(server: &FakeHttp, judge: &CountingJudge) -> Option<JudgedImage> {
    let slot = JudgeSlot {
        query: "serum dropper",
        intent: "",
        judge,
        view: None,
        deadline: None,
    };
    let used = Mutex::new(HashSet::new());
    crate::net::block_on_image_runtime(fetch_judged_wikimedia(
        &test_client(),
        &server.base_url,
        "serum dropper",
        &slot,
        &used,
        true,
    ))
}

#[test]
fn refused_license_never_reaches_the_judge_on_the_judged_wikimedia_path() {
    let nc_only = commons_with(vec![("CC BY-NC 2.0", "")]);
    let judge = CountingJudge::default();
    assert!(judged(&nc_only, &judge).is_none());
    assert_eq!(judge.thumbs_seen.load(Ordering::SeqCst), 0);
    assert!(
        nc_only
            .requests()
            .iter()
            .all(|target| !target.starts_with("/thumb/")),
        "a refused page must not even be downloaded"
    );

    let mixed = commons_with(vec![("CC BY-NC 2.0", ""), ("CC BY 4.0", "")]);
    let judge = CountingJudge::default();
    let winner = judged(&mixed, &judge).expect("the CC BY page wins");
    assert_eq!(judge.thumbs_seen.load(Ordering::SeqCst), 1);
    let provenance = winner.provenance.expect("provenance");
    assert_eq!(provenance.provider, "wikimedia");
    assert_eq!(provenance.license.as_deref(), Some("CC BY 4.0"));
    assert_eq!(provenance.creator.as_deref(), Some("Jane & Co"));
}
