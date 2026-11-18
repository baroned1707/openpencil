use super::*;

fn view(input: u64, cache_read: u64, cache_write: u64, output: u64) -> UsageView {
    UsageView {
        input,
        cache_read,
        cache_write,
        output,
        turns: 1,
    }
}

#[test]
fn usage_view_formats_k_units_and_cache_share() {
    assert_eq!(format_token_count(0), "0");
    assert_eq!(format_token_count(999), "999");
    assert_eq!(format_token_count(1_000), "1.0k");
    assert_eq!(format_token_count(48_249), "48.2k");
    assert_eq!(format_token_count(48_250), "48.3k");
    assert_eq!(format_token_count(3_100), "3.1k");
    // Rounding up to the next unit never prints `1000.0k`.
    assert_eq!(format_token_count(999_949), "999.9k");
    assert_eq!(format_token_count(999_950), "1.0M");
    assert_eq!(format_token_count(1_250_000), "1.3M");

    // 12 fresh + 3000 written + 45000 read: in = 48012, cached = 45000 / 48012.
    let run = view(12, 45_000, 3_000, 210);
    assert_eq!(run.total_input(), 48_012);
    assert_eq!(run.cached_percent(), Some(94));
    // The denominator includes cache writes and fresh input, not read alone.
    let run = view(5_000, 4_000, 1_000, 10);
    assert_eq!(run.cached_percent(), Some(40));
    assert_eq!(view(0, 100, 0, 1).cached_percent(), Some(100));
}

#[test]
fn cache_share_hidden_without_cache_data() {
    assert_eq!(view(48_200, 0, 0, 3_100).cached_percent(), None);
    assert!(!view(48_200, 0, 0, 3_100).has_cache_data());
    // A run that only wrote the cache has cache data: a real 0% hit rate.
    let wrote_only = view(100, 0, 900, 5);
    assert!(wrote_only.has_cache_data());
    assert_eq!(wrote_only.cached_percent(), Some(0));
    // Output-only data (no input reported) cannot divide by zero.
    assert_eq!(view(0, 0, 0, 5).cached_percent(), None);
}

#[test]
fn usage_is_cleared_when_a_new_run_starts() {
    let mut chat = ChatState::default();
    chat.record_run_usage(view(10, 0, 0, 5));
    assert!(chat.last_run_usage.is_some());
    chat.set_input_text("next");
    assert!(chat.begin_send());
    assert_eq!(chat.last_run_usage, None, "begin_send starts a new run");

    chat.record_run_usage(view(10, 0, 0, 5));
    chat.new_chat();
    assert_eq!(chat.last_run_usage, None, "a fresh chat has no usage");
}

#[test]
fn usage_of_agents_in_one_turn_is_summed() {
    let mut chat = ChatState::default();
    chat.record_run_usage(view(10, 100, 0, 5));
    chat.record_run_usage(view(20, 200, 30, 7));
    let total = chat.last_run_usage.expect("recorded");
    assert_eq!(
        total,
        UsageView {
            input: 30,
            cache_read: 300,
            cache_write: 30,
            output: 12,
            turns: 2,
        }
    );
}
