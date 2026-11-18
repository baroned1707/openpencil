//! Process-wide per-provider rate-limit pacing and Openverse OAuth token reuse.
//!
//! State is keyed by the provider's base URL, so two providers (or a real
//! host and a loopback test fake) never share a quota flag. Openverse keeps
//! its `x-ratelimit-*` / `Retry-After` / daily-quota logic; Commons gets
//! serial requests, exponential backoff on 429/503 that honours
//! `Retry-After`, and a skip-for-the-process rule after repeated throttling.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, RETRY_AFTER};

const DEFAULT_RETRY_AFTER_SECONDS: u64 = 60;
const MAX_SLEEP_SECONDS: u64 = 65;
const TOKEN_EXPIRY_MARGIN_SECONDS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedRateLimits {
    pub(crate) burst_available: Option<u64>,
    pub(crate) burst_window: Option<Duration>,
    pub(crate) sustained_available: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedToken {
    token: String,
    valid_until: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct TokenCache {
    entries: HashMap<String, CachedToken>,
}

impl TokenCache {
    pub(crate) fn insert_at(
        &mut self,
        client_id: &str,
        token: String,
        expires_in: u64,
        now: Instant,
    ) {
        let valid_for = expires_in.saturating_sub(TOKEN_EXPIRY_MARGIN_SECONDS);
        self.entries.insert(
            client_id.to_string(),
            CachedToken {
                token,
                valid_until: now + Duration::from_secs(valid_for),
            },
        );
    }

    pub(crate) fn get_at(&self, client_id: &str, now: Instant) -> Option<&str> {
        self.entries
            .get(client_id)
            .filter(|entry| now < entry.valid_until)
            .map(|entry| entry.token.as_str())
    }

    pub(crate) fn remove(&mut self, client_id: &str) {
        self.entries.remove(client_id);
    }
}

/// Rate-limit state of one provider host.
#[derive(Debug, Default)]
struct ProviderPacing {
    burst_window_until: Option<Instant>,
    daily_quota_exhausted: bool,
    daily_quota_logged: bool,
    /// Consecutive 429/503 answers (reset by any success).
    consecutive_throttled: u32,
    /// Set once throttling repeats: no more requests this process.
    skipped: bool,
}

#[derive(Debug, Default)]
pub(crate) struct PacingState {
    providers: HashMap<String, ProviderPacing>,
    pub(crate) token_cache: TokenCache,
    token_locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
    serial_locks: HashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

impl PacingState {
    fn provider(&mut self, key: &str) -> &mut ProviderPacing {
        self.providers.entry(key.to_string()).or_default()
    }
}

static STATE: OnceLock<Mutex<PacingState>> = OnceLock::new();

pub(crate) fn state() -> &'static Mutex<PacingState> {
    STATE.get_or_init(|| Mutex::new(PacingState::default()))
}

/// Three throttled answers in a row mean the host does not want us; stop for
/// the rest of the process (same spirit as Openverse's daily-quota stop).
const SKIP_AFTER_CONSECUTIVE_THROTTLES: u32 = 3;

/// Retry schedule for a throttled provider: `base * 2^attempt` capped at
/// `max`, replaced by the server's `Retry-After` (also capped) when present.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BackoffPolicy {
    pub(crate) base: Duration,
    pub(crate) max: Duration,
    /// Retries after the first request.
    pub(crate) retries: u32,
}

impl BackoffPolicy {
    pub(crate) const DEFAULT: Self = Self {
        base: Duration::from_secs(1),
        max: Duration::from_secs(MAX_SLEEP_SECONDS),
        retries: 2,
    };

    /// Delay before retry number `attempt` (0-based).
    pub(crate) fn delay(&self, attempt: u32, headers: &HeaderMap) -> Duration {
        let server = headers
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        server
            .unwrap_or_else(|| self.base.saturating_mul(2u32.saturating_pow(attempt)))
            .min(self.max)
    }
}

/// One-at-a-time gate for a host that must never see parallel requests.
pub(crate) fn serial_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut state = state().lock().expect("pacing mutex poisoned");
    Arc::clone(
        state
            .serial_locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

pub(crate) fn is_skipped(key: &str) -> bool {
    state()
        .lock()
        .expect("pacing mutex poisoned")
        .provider(key)
        .skipped
}

/// Count one 429/503. Returns `true` when this answer tipped the provider
/// into the skipped-for-the-process state.
pub(crate) fn note_throttled(key: &str) -> bool {
    let mut state = state().lock().expect("pacing mutex poisoned");
    let pacing = state.provider(key);
    pacing.consecutive_throttled += 1;
    if pacing.consecutive_throttled >= SKIP_AFTER_CONSECUTIVE_THROTTLES && !pacing.skipped {
        pacing.skipped = true;
        eprintln!("[ENRICH] {key}: repeatedly throttled, skipping this provider for this process");
    }
    pacing.skipped
}

pub(crate) fn note_success(key: &str) {
    state()
        .lock()
        .expect("pacing mutex poisoned")
        .provider(key)
        .consecutive_throttled = 0;
}

pub(crate) fn parse_rate_limit_headers(headers: &HeaderMap) -> ParsedRateLimits {
    let mut burst_available = None;
    let mut burst_window = None;
    let mut sustained_available = None;

    for (name, value) in headers {
        let name = name.as_str().to_ascii_lowercase();
        let Ok(value) = value.to_str() else {
            continue;
        };
        if name.starts_with("x-ratelimit-available-") && name.ends_with("burst") {
            burst_available = value.trim().parse().ok();
        } else if name.starts_with("x-ratelimit-limit-") && name.ends_with("burst") {
            burst_window = parse_limit_window(value);
        } else if name.starts_with("x-ratelimit-available-") && name.ends_with("sustained") {
            sustained_available = value.trim().parse().ok();
        }
    }

    ParsedRateLimits {
        burst_available,
        burst_window,
        sustained_available,
    }
}

fn parse_limit_window(value: &str) -> Option<Duration> {
    let (count, unit) = value.trim().split_once('/')?;
    count.trim().parse::<u64>().ok()?;
    let seconds = match unit.trim().to_ascii_lowercase().as_str() {
        "s" | "sec" | "second" | "seconds" => 1,
        "m" | "min" | "minute" | "minutes" => 60,
        "h" | "hour" | "hours" => 60 * 60,
        "d" | "day" | "days" => 60 * 60 * 24,
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

pub(crate) fn retry_after_seconds(headers: &HeaderMap) -> u64 {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_RETRY_AFTER_SECONDS)
}

pub(crate) fn burst_wait_duration(headers: &HeaderMap) -> Option<Duration> {
    let parsed = parse_rate_limit_headers(headers);
    (parsed.burst_available == Some(0)).then_some(
        parsed
            .burst_window
            .unwrap_or_else(|| Duration::from_secs(60)),
    )
}

pub(crate) fn record_response(key: &str, headers: &HeaderMap) {
    let parsed = parse_rate_limit_headers(headers);
    let now = Instant::now();
    let mut state = state().lock().expect("Openverse pacing mutex poisoned");
    let state = state.provider(key);

    if parsed.sustained_available == Some(0) {
        state.daily_quota_exhausted = true;
        if !state.daily_quota_logged {
            state.daily_quota_logged = true;
            eprintln!(
                "[ENRICH] openverse: daily quota exhausted, skipping Openverse for this process"
            );
        }
    }
    if let Some(window) = burst_wait_duration(headers) {
        let until = now + window;
        state.burst_window_until = Some(
            state
                .burst_window_until
                .map_or(until, |existing| existing.max(until)),
        );
    }
}

pub(crate) async fn wait_for_request(key: &str) -> bool {
    loop {
        let wait_for = {
            let mut state = state().lock().expect("Openverse pacing mutex poisoned");
            let state = state.provider(key);
            if state.daily_quota_exhausted {
                return false;
            }
            state
                .burst_window_until
                .and_then(|until| until.checked_duration_since(Instant::now()))
        };
        let Some(wait_for) = wait_for else {
            return true;
        };
        if wait_for.is_zero() {
            return true;
        }
        let sleep_for = wait_for.min(Duration::from_secs(MAX_SLEEP_SECONDS));
        let wait_seconds = sleep_for.as_secs() + u64::from(sleep_for.subsec_nanos() != 0);
        eprintln!("[ENRICH] openverse: burst limit reached, waiting {wait_seconds}s");
        tokio::time::sleep(sleep_for).await;
    }
}

pub(crate) async fn retry_delay(headers: &HeaderMap) {
    tokio::time::sleep(
        Duration::from_secs(retry_after_seconds(headers))
            .min(Duration::from_secs(MAX_SLEEP_SECONDS)),
    )
    .await;
}

pub(crate) fn token_lock(client_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut state = state().lock().expect("Openverse pacing mutex poisoned");
    Arc::clone(
        state
            .token_locks
            .entry(client_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

pub(crate) fn cached_token(client_id: &str) -> Option<String> {
    let state = state().lock().expect("Openverse pacing mutex poisoned");
    state
        .token_cache
        .get_at(client_id, Instant::now())
        .map(str::to_string)
}

pub(crate) fn cache_token(client_id: &str, token: String, expires_in: u64) {
    let mut state = state().lock().expect("Openverse pacing mutex poisoned");
    state
        .token_cache
        .insert_at(client_id, token, expires_in, Instant::now());
}

pub(crate) fn drop_cached_token(client_id: &str) {
    let mut state = state().lock().expect("Openverse pacing mutex poisoned");
    state.token_cache.remove(client_id);
}

#[cfg(test)]
#[path = "pacing_tests.rs"]
mod tests;
