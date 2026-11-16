use super::*;
use std::collections::BTreeMap;

fn fingerprint(tool: &str, pairs: &[(&str, &str)]) -> CallFingerprint {
    let arguments: BTreeMap<String, String> = pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    CallFingerprint::of(tool, &arguments)
}

#[test]
fn operation_id_cache_is_bounded_and_expires() {
    let start = Instant::now();
    let same = fingerprint("batch_design", &[("script", "x")]);
    let mut log = OperationLog::default();

    // Bounded: the 257th distinct operation evicts the least recently used.
    for index in 0..OPERATION_LOG_CAPACITY {
        log.record(&format!("op-{index}"), same, format!("r{index}"), start);
    }
    assert_eq!(log.len(), OPERATION_LOG_CAPACITY);
    // Touch op-0 so op-1 becomes the least recently used.
    assert!(matches!(log.lookup("op-0", same, start), Lookup::Replay(_)));
    log.record("op-new", same, "new".into(), start);
    assert_eq!(log.len(), OPERATION_LOG_CAPACITY);
    assert_eq!(log.lookup("op-1", same, start), Lookup::Miss);
    assert!(matches!(log.lookup("op-0", same, start), Lookup::Replay(_)));
    assert!(matches!(
        log.lookup("op-new", same, start),
        Lookup::Replay(_)
    ));

    // Expires: past the TTL nothing replays and the log empties.
    let later = start + OPERATION_LOG_TTL + Duration::from_secs(1);
    assert_eq!(log.lookup("op-0", same, later), Lookup::Miss);
    assert_eq!(log.len(), 0);
}

#[test]
fn an_entry_just_inside_the_ttl_still_replays() {
    let start = Instant::now();
    let call = fingerprint("undo", &[]);
    let mut log = OperationLog::default();
    log.record("op", call, "stored".into(), start);
    let almost = start + OPERATION_LOG_TTL - Duration::from_secs(1);
    assert_eq!(
        log.lookup("op", call, almost),
        Lookup::Replay("stored".into())
    );
}

#[test]
fn a_reused_id_with_different_arguments_is_a_conflict() {
    let now = Instant::now();
    let mut log = OperationLog::default();
    log.record(
        "op",
        fingerprint("set_node_name", &[("name", "A")]),
        "r".into(),
        now,
    );
    assert_eq!(
        log.lookup("op", fingerprint("set_node_name", &[("name", "B")]), now),
        Lookup::Conflict
    );
    assert_eq!(
        log.lookup("op", fingerprint("set_node_text", &[("name", "A")]), now),
        Lookup::Conflict
    );
}

#[test]
fn replay_response_restamps_the_id_and_marks_replayed() {
    let stored = r#"{"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"ok"}],"isError":false}}"#;
    let replayed = replay_response(stored, &op_mcp::RequestId::Num(9));
    let value: serde_json::Value = serde_json::from_str(&replayed).expect("json");
    assert_eq!(value["id"], 9);
    assert_eq!(value["result"]["_meta"]["replayed"], true);
    assert_eq!(value["result"]["content"][0]["text"], "ok");
    let named = replay_response(stored, &op_mcp::RequestId::Str("req-7".into()));
    assert!(named.contains(r#""id":"req-7""#), "{named}");
}
