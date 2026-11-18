//! Style suggestions before sending: while the user types a design prompt and
//! has no style pinned, the composer offers up to three matching styles.
//!
//! The data is the embedded style table (native-only), so this lives in the
//! desktop host: the web host and mobile never fill `chat.style_suggestions` and
//! draw nothing. The search is in memory and takes microseconds, so there is no
//! worker thread; the only cost control is the debounce: the text must sit
//! unchanged for [`DEBOUNCE_MS`] before it is searched, and a text is searched
//! once (`chat.suggestions_for` remembers it, empty result included).
//!
//! Two kinds of gate:
//! - hard gates (a pinned style, a design.md, an empty composer, a run in
//!   flight) clear the row at once, every frame, with no search;
//! - text gates (long enough, a NEW design request, a canvas that is empty or a
//!   request for a new screen) are applied inside the debounced search.

use op_editor_core::{style_blurb, text_fingerprint, EditorState, StyleSuggestion};
use op_host_services::chat_intent::{
    classify_by_keywords, has_new_screen_creation_signal, is_non_request_text,
    requests_new_whole_screen, DesignIntent,
};
use op_orchestrator::{classify_intent, Intent};

/// How long the composer text must stay unchanged before it is searched.
pub(crate) const DEBOUNCE_MS: u64 = 350;
/// Shortest prompt that is searched, in characters (words are counted too).
const MIN_CHARS: usize = 12;
const MIN_WORDS: usize = 3;
/// CJK prompts have no spaces, so they are measured in characters only.
const MIN_CJK_CHARS: usize = 6;
/// Lowest score a product-type match needs before its styles are offered.
/// Calibrated on the embedded tables: a single stray keyword scores 10 (the
/// search floor) and matches unrelated products ("create a login page" finds a
/// beat maker); two clear keywords score 16 and up.
const MIN_PRODUCT_SCORE: u32 = 16;
/// Lowest score of a style found by the text itself, with no product match.
/// A style the text names scores 22; one keyword (the search floor of 10)
/// matches whole families of styles ("dashboard" fits five) and says nothing.
const MIN_STYLE_SCORE: u32 = 20;
/// Product hits looked at, best first.
const PRODUCT_HITS: usize = 3;

/// Debounce state of the one composer on screen.
#[derive(Debug, Default)]
pub(crate) struct StyleSuggestDriver {
    /// The text fingerprint waiting out the debounce, and when it was first seen.
    pending: Option<(u64, u64)>,
    /// How many searches ran. Tests read it to prove typing does not search
    /// every frame.
    pub(crate) searches: usize,
}

impl StyleSuggestDriver {
    /// Run once per frame. Returns whether the row's content changed (the
    /// caller then repaints).
    pub(crate) fn tick(&mut self, state: &mut EditorState, now_ms: u64) -> bool {
        // A run in flight or queued owns the composer: `begin_send` already
        // cleared the chips, and the launcher takes the pick.
        if state.chat.has_streaming_turn() {
            self.pending = None;
            return false;
        }
        let text = state.chat.input.effective_text().to_string();
        let blocked = state.editor_ui.pinned_style_guide.is_some() || state.doc.design_md.is_some();
        if blocked || text.trim().is_empty() {
            self.pending = None;
            return clear_row(state);
        }
        let fingerprint = text_fingerprint(&text);
        if state.chat.suggestions_for == Some(fingerprint) {
            self.pending = None;
            return false;
        }
        let since = match self.pending {
            Some((pending, since)) if pending == fingerprint => since,
            _ => {
                self.pending = Some((fingerprint, now_ms));
                return false;
            }
        };
        if now_ms.saturating_sub(since) < DEBOUNCE_MS {
            return false;
        }
        self.pending = None;
        self.searches += 1;
        let found = suggest_styles(state, &text);
        let before = state.chat.style_suggestions.clone();
        state.chat.set_style_suggestions(fingerprint, found);
        state.chat.style_suggestions != before
    }

    /// When the debounce of the waiting text elapses, so the event loop wakes
    /// for it even though nothing else is happening.
    pub(crate) fn next_deadline_ms(&self) -> Option<u64> {
        self.pending.map(|(_, since)| since + DEBOUNCE_MS)
    }
}

impl crate::DesktopApp {
    /// The per-frame hook (`prepare_redraw`): advances the debounce for the
    /// composer on screen. Returns whether the row changed.
    pub(crate) fn tick_style_suggestions(&mut self, now_ms: u64) -> bool {
        let changed = self
            .style_suggest
            .tick(self.host.editor_state_mut(), now_ms);
        if changed {
            self.host.mark_editor_state_dirty();
        }
        changed
    }

    /// The instant the waiting text's debounce elapses, if one is waiting.
    pub(crate) fn style_suggest_wake(&self) -> Option<std::time::Instant> {
        self.style_suggest
            .next_deadline_ms()
            .map(|ms| self.clock_start + std::time::Duration::from_millis(ms))
    }
}

/// Forget the row for a composer that may not offer styles. Returns whether
/// anything visible changed.
fn clear_row(state: &mut EditorState) -> bool {
    let chat = &mut state.chat;
    let visible = chat.has_style_suggest_row();
    chat.clear_style_suggestions();
    chat.chosen_style = None;
    chat.suggestions_dismissed_for = None;
    visible
}

/// The styles to offer for `text`, up to three, or none. Applies the text
/// gates; the hard gates are the caller's.
pub(crate) fn suggest_styles(state: &EditorState, text: &str) -> Vec<StyleSuggestion> {
    if !is_long_enough(text)
        || !looks_like_new_design_request(text)
        || !canvas_wants_new_design(state, text)
    {
        return Vec::new();
    }
    let mut refs: Vec<&'static op_ai_skills::StyleRecord> = Vec::new();
    let mut push = |record: &'static op_ai_skills::StyleRecord| {
        if !refs.iter().any(|known| known.reference == record.reference) {
            refs.push(record);
        }
    };
    let strong_styles: Vec<_> = op_ai_skills::search_styles(text, 3)
        .into_iter()
        .filter(|hit| hit.score >= MIN_STYLE_SCORE)
        .collect();
    // A style the user names in the text leads: offering it is the minimum.
    for hit in &strong_styles {
        push(hit.record);
    }
    let products = op_ai_skills::search_design_data(text, None, PRODUCT_HITS);
    let floor = products.first().map_or(MIN_PRODUCT_SCORE, |top| {
        MIN_PRODUCT_SCORE.max(top.score / 2)
    });
    for hit in products.iter().filter(|hit| hit.score >= floor) {
        for name in &hit.record.styles {
            if let Some(record) = name
                .starts_with("external:")
                .then(|| op_ai_skills::style_by_ref(name))
                .flatten()
            {
                push(record);
            }
        }
    }
    refs.into_iter()
        .take(op_editor_core::MAX_STYLE_SUGGESTIONS)
        .map(|record| StyleSuggestion {
            style_ref: record.reference.clone(),
            name: record.name.clone(),
            blurb: record
                .best_for
                .as_deref()
                .map(style_blurb)
                .unwrap_or_default(),
        })
        .collect()
}

/// At least 12 characters and 3 words; a CJK prompt (no spaces) needs 6
/// characters.
fn is_long_enough(text: &str) -> bool {
    let trimmed = text.trim();
    let chars = trimmed.chars().filter(|c| !c.is_whitespace()).count();
    if trimmed.chars().any(is_cjk) {
        return chars >= MIN_CJK_CHARS;
    }
    trimmed.chars().count() >= MIN_CHARS && trimmed.split_whitespace().count() >= MIN_WORDS
}

fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}')
}

/// The two keyword helpers the send path itself routes with, no LLM: the
/// standard-route classifier must read it as a new design (not an edit and not
/// a question), and the design-verb gate that sends a turn to the design agent
/// must read it as design. `classify_by_keywords` falls through to "new", so the
/// second one is the positive evidence.
fn looks_like_new_design_request(text: &str) -> bool {
    !is_non_request_text(text)
        && classify_by_keywords(text) == DesignIntent::New
        && classify_intent(text) == Intent::Design
}

/// Styles suit a first design: an empty canvas (or the blank starter frame), or
/// a prompt that asks for a new screen. A prompt that works on what is already
/// drawn gets none.
fn canvas_wants_new_design(state: &EditorState, text: &str) -> bool {
    state.active_children().is_empty()
        || op_editor_core::blank_starter::active_page_is_blank_starter(state)
        || requests_new_whole_screen(text)
        || has_new_screen_creation_signal(text)
}

#[cfg(test)]
#[path = "style_suggest_host_tests.rs"]
mod tests;
