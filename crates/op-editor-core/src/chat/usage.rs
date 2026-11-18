//! Token usage of the last agent run, as the chat panel shows it.
//!
//! Plain numbers only: the host converts the provider's `RunUsage` into this
//! view when it drains the delta, so this crate stays free of provider types.
//! The view lives on the chat tab ([`ChatState::last_run_usage`]), in memory
//! only: it is never written to a document or to settings.
//!
//! [`ChatState::last_run_usage`]: super::ChatState::last_run_usage

use super::ChatState;

/// What one run (or the sum of one turn's runs) spent, in tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageView {
    /// Fresh input tokens: not read from and not written to the prompt cache.
    pub input: u64,
    /// Input tokens served from the prompt cache.
    pub cache_read: u64,
    /// Input tokens written to the prompt cache.
    pub cache_write: u64,
    pub output: u64,
    /// Responses the run made.
    pub turns: u32,
}

impl UsageView {
    /// Every input token the run sent: fresh, cache read and cache written.
    /// This is the "in" of the meter and the denominator of the cache share.
    pub fn total_input(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// Whether the run reported any cache activity at all.
    pub fn has_cache_data(&self) -> bool {
        self.cache_read > 0 || self.cache_write > 0
    }

    /// Share of the input served from the cache, as a whole percent. `None`
    /// when the run reported no cache data (the meter then omits the share
    /// rather than claiming 0%). A run that only wrote the cache is a real 0%.
    pub fn cached_percent(&self) -> Option<u8> {
        if !self.has_cache_data() {
            return None;
        }
        let total = self.total_input();
        // `has_cache_data` guarantees a non-zero total.
        let percent =
            (u128::from(self.cache_read) * 100 + u128::from(total) / 2) / u128::from(total);
        Some(percent.min(100) as u8)
    }

    /// Add another run's numbers to this one.
    pub fn add(&mut self, other: &UsageView) {
        self.input = self.input.saturating_add(other.input);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.output = self.output.saturating_add(other.output);
        self.turns = self.turns.saturating_add(other.turns);
    }
}

/// A token count for the meter: exact below 1,000, then thousands with one
/// decimal (`48.2k`), then millions with one decimal (`1.3M`). Counts that
/// would round up to the next unit use it (`999,960` is `1.0M`, not `1000.0k`).
pub fn format_token_count(count: u64) -> String {
    if count < 1_000 {
        return count.to_string();
    }
    // Tenths of a thousand, rounded half up.
    let tenths_of_thousands = (count + 50) / 100;
    if tenths_of_thousands < 10_000 {
        return format!("{}.{}k", tenths_of_thousands / 10, tenths_of_thousands % 10);
    }
    let tenths_of_millions = (count + 50_000) / 100_000;
    format!("{}.{}M", tenths_of_millions / 10, tenths_of_millions % 10)
}

impl ChatState {
    /// Fold one run's usage into this tab's meter. A turn that launches
    /// several tool-loop agents (a team) reports each one, and the meter shows
    /// their sum. Orchestrator turns report none: their provider adapter does
    /// not forward usage. [`Self::begin_send`] starts the next sum.
    ///
    /// [`Self::begin_send`]: ChatState::begin_send
    pub fn record_run_usage(&mut self, usage: UsageView) {
        match self.last_run_usage.as_mut() {
            Some(total) => total.add(&usage),
            None => self.last_run_usage = Some(usage),
        }
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
