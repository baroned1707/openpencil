//! Style suggestions shown above the composer while a design prompt is typed.
//!
//! The host (desktop only: the style table is native-only data) turns the
//! typed text into up to three [`StyleSuggestion`]s. The user may pick one; the
//! pick is a one-run choice that rides the next design request and is then
//! forgotten. Plain data only, so this crate stays free of the style tables.
//!
//! Everything here lives on the chat tab ([`ChatState`]), in memory only: it
//! moves with the tab (and so with its document) and is never written to a
//! document or to settings.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::ChatState;

/// Most chips the row offers.
pub const MAX_STYLE_SUGGESTIONS: usize = 3;
/// Longest blurb, in characters, before it is cut at a word boundary.
pub const STYLE_BLURB_MAX_CHARS: usize = 70;

/// One suggested (or chosen) style: the reference the style table resolves,
/// its display name and one short line on what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleSuggestion {
    /// The style table's `ref` (`minimalism-and-swiss-style`).
    pub style_ref: String,
    pub name: String,
    /// What the style is best for, at most [`STYLE_BLURB_MAX_CHARS`] characters.
    pub blurb: String,
}

/// `best_for` cut to [`STYLE_BLURB_MAX_CHARS`] characters at a word boundary,
/// with an ellipsis only when something was cut.
pub fn style_blurb(best_for: &str) -> String {
    let text = best_for.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= STYLE_BLURB_MAX_CHARS {
        return text;
    }
    // Leave room for the ellipsis so the whole blurb stays within the limit.
    let room = STYLE_BLURB_MAX_CHARS - 1;
    let head: String = text.chars().take(room).collect();
    // A cut that lands exactly before a space already ends on a whole word.
    let next_is_space = text.chars().nth(room) == Some(' ');
    let cut = if next_is_space {
        head.as_str()
    } else {
        head.rsplit_once(' ')
            .map_or(head.as_str(), |(whole, _)| whole)
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '-', ' ']))
}

/// Fingerprint of a prompt, used to say "this exact text" without keeping it.
pub fn text_fingerprint(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl ChatState {
    /// Replace the offered chips for the text fingerprinted as `for_text`.
    /// At most [`MAX_STYLE_SUGGESTIONS`] are kept, and the list is empty when
    /// the user dismissed the row for exactly this text.
    pub fn set_style_suggestions(&mut self, for_text: u64, mut suggestions: Vec<StyleSuggestion>) {
        if self.suggestions_dismissed_for == Some(for_text) {
            suggestions.clear();
        } else {
            self.suggestions_dismissed_for = None;
        }
        suggestions.truncate(MAX_STYLE_SUGGESTIONS);
        self.style_suggestions = suggestions;
        self.suggestions_for = Some(for_text);
    }

    /// Drop the offered chips (the choice, if any, stays).
    pub fn clear_style_suggestions(&mut self) {
        self.style_suggestions.clear();
        self.suggestions_for = None;
    }

    /// Pick the suggestion at `index` for the next run; picking the chosen
    /// style again lets it go. Returns whether anything changed.
    pub fn toggle_chosen_style(&mut self, index: usize) -> bool {
        // `index` counts the chips as the row shows them (`style_suggest_chips`):
        // a lone pick is chip 0 of its own row.
        let Some(picked) = self.style_suggest_chips().get(index).copied().cloned() else {
            return false;
        };
        if self.chosen_style.as_ref() == Some(&picked) {
            self.chosen_style = None;
        } else {
            self.chosen_style = Some(picked);
        }
        true
    }

    /// The row's dismiss: hide the chips and forget the choice until the text
    /// changes. Returns whether anything changed.
    pub fn dismiss_style_suggestions(&mut self) -> bool {
        let had_row = self.has_style_suggest_row();
        self.suggestions_dismissed_for = Some(text_fingerprint(&self.input.effective_text()));
        self.style_suggestions.clear();
        self.suggestions_for = self.suggestions_dismissed_for;
        self.chosen_style = None;
        had_row
    }

    /// Whether the suggestion row has anything to show: chips to pick from or a
    /// pick that will apply to the next run (it must stay visible then).
    pub fn has_style_suggest_row(&self) -> bool {
        !self.style_suggestions.is_empty() || self.chosen_style.is_some()
    }

    /// The chips the row paints, in order: the offered list, or the lone
    /// choice when the offered list no longer contains it.
    pub fn style_suggest_chips(&self) -> Vec<&StyleSuggestion> {
        match &self.chosen_style {
            Some(chosen) if !self.style_suggestions.contains(chosen) => vec![chosen],
            _ => self.style_suggestions.iter().collect(),
        }
    }

    /// A run starts: the offered chips no longer describe the (now empty)
    /// composer. The choice stays until the launcher consumes it.
    pub(super) fn style_run_started(&mut self) {
        self.clear_style_suggestions();
    }
}

#[cfg(test)]
#[path = "style_suggestion_tests.rs"]
mod tests;
