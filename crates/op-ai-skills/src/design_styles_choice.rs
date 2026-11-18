//! The paragraph that tells an agent which style the user picked for ONE run.
//!
//! The chat composer offers up to three styles while a design prompt is typed
//! (`StyleSuggestion` in `op-editor-core` keeps only the style's reference); the
//! picked one reaches the in-app design loop and the orchestrator as this one
//! compact paragraph, rendered here from the embedded style record so both
//! routes say the same thing. It is guidance from the user, so it says what
//! outranks it: the project's design.md and a pinned style guide.

use crate::design_styles::{style_by_ref, StyleRecord};
use crate::design_styles_search::short_list;

/// Longest paragraph returned, in bytes.
pub const MAX_CHOSEN_STYLE_BYTES: usize = 1200;
/// Longest single field, in characters, before the byte cap applies.
const FIELD_MAX_CHARS: usize = 200;

/// The sentence that opens the paragraph. A test pins the precedence wording.
pub const CHOSEN_STYLE_MARKER: &str = "The user chose this style for this run: follow it unless \
they say otherwise; design.md and the pinned style (if any) outrank it.";

/// The paragraph for the style named `style_ref`, or `None` when no such style
/// exists (a table that did not load, or a reference that was retired): then
/// the run simply carries no choice.
pub fn chosen_style_paragraph(style_ref: &str) -> Option<String> {
    style_by_ref(style_ref).map(render)
}

fn render(record: &StyleRecord) -> String {
    let mut text = format!("{CHOSEN_STYLE_MARKER}\nStyle: {}", record.name);
    if let Some(category) = &record.category {
        text.push_str(&format!(" ({category})"));
    }
    text.push('.');
    let mut fields: Vec<(&str, String)> = Vec::new();
    for (label, value) in [
        ("Best for", &record.best_for),
        ("Do not use for", &record.do_not_use_for),
        ("Effects", &record.effects),
        ("Accessibility", &record.accessibility),
        ("Light/dark", &record.light_dark_mode),
    ] {
        if let Some(value) = value {
            fields.push((label, short_list(value, FIELD_MAX_CHARS)));
        }
    }
    for (label, colors) in [
        ("Primary colours", &record.primary_colors),
        ("Secondary colours", &record.secondary_colors),
    ] {
        if !colors.is_empty() {
            fields.push((label, colors.join(", ")));
        }
    }
    // Fields are added in priority order; one that no longer fits is skipped,
    // never cut mid-text, so the paragraph is always whole sentences.
    for (label, value) in fields {
        let line = format!(" {label}: {value}.");
        if text.len() + line.len() <= MAX_CHOSEN_STYLE_BYTES {
            text.push_str(&line);
        }
    }
    text
}

#[cfg(test)]
#[path = "design_styles_choice_tests.rs"]
mod tests;
