//! Search over the embedded style table, the compact JSON `get_guidelines`
//! returns for `category: "data", kind: "style"`, and the short style
//! summaries attached to product matches.
//!
//! Scoring mirrors [`crate::design_data_search`] (same keyword matcher, same
//! weights, same threshold): a query that NAMES a style (its ref, id or display
//! name, with or without the `external:` prefix of a product record) ranks
//! first; otherwise keywords and name words decide, equal scores order by id,
//! and a weak match returns NOTHING rather than the least-bad style.

use std::cmp::Reverse;

use serde_json::{json, Map, Value};

use crate::design_data::EXTERNAL_STYLE_PREFIX;
use crate::design_data_search::{
    count_shared, envelope, significant_words, EXTRA_KEYWORD_WORD_WEIGHT, KEYWORD_WEIGHT,
    KEYWORD_WORD_WEIGHT, MAX_LIMIT, MAX_QUERY_CHARS, MAX_RESPONSE_BYTES, MIN_SCORE,
    NAME_PHRASE_WEIGHT,
};
use crate::design_styles::{
    find_style, names_style, strip_external, style_records, words_key, StyleRecord,
};
use crate::resolver::match_keyword;

/// Words that appear in many style names and say nothing about one style.
const STYLE_GENERIC_WORDS: &[&str] = &[
    "style",
    "mobile",
    "system",
    "first",
    "touch",
    "based",
    "inspired",
    "interface",
    "mode",
    "dark",
    "light",
    "dimensional",
    "modern",
    "digital",
];

/// Most style summaries attached to one product match.
pub const MAX_SUMMARIES: usize = 3;
/// Longest `bestFor` / `doNotUseFor` text in a summary.
pub const MAX_SUMMARY_CHARS: usize = 120;

/// One search result: a style record and its score.
#[derive(Debug, Clone, Copy)]
pub struct StyleHit {
    pub record: &'static StyleRecord,
    pub score: u32,
}

/// Best styles for `query`, best first (at most `limit`). Empty when nothing
/// names a style and nothing reaches [`MIN_SCORE`].
pub fn search_styles(query: &str, limit: usize) -> Vec<StyleHit> {
    search_style_records(style_records(), query, limit)
}

pub(crate) fn search_style_records(
    records: &'static [StyleRecord],
    query: &str,
    limit: usize,
) -> Vec<StyleHit> {
    let query: String = query.chars().take(MAX_QUERY_CHARS).collect();
    let query = strip_external(&query);
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let words = significant_words(&query);
    let mut hits: Vec<(bool, StyleHit)> = records
        .iter()
        .filter_map(|record| {
            let exact = names_style(record, &query);
            let score = style_score(record, &query, &words);
            (exact || score >= MIN_SCORE).then_some((exact, StyleHit { record, score }))
        })
        .collect();
    hits.sort_by(|(exact_a, a), (exact_b, b)| {
        (Reverse(*exact_a), Reverse(a.score), &a.record.id).cmp(&(
            Reverse(*exact_b),
            Reverse(b.score),
            &b.record.id,
        ))
    });
    hits.truncate(limit);
    hits.into_iter().map(|(_, hit)| hit).collect()
}

/// Keyword and name evidence; the only part that decides match / no match.
fn style_score(record: &StyleRecord, query: &str, words: &[String]) -> u32 {
    let mut score = 0;
    for keyword in &record.keywords {
        if match_keyword(query, keyword) {
            let extra_words = keyword.split_whitespace().count().saturating_sub(1) as u32;
            score += KEYWORD_WEIGHT + EXTRA_KEYWORD_WORD_WEIGHT * extra_words;
        }
    }
    let spaced_query = words_key(query);
    for phrase in [words_key(&record.name), words_key(&record.reference)] {
        if !phrase.is_empty() && match_keyword(&spaced_query, &phrase) {
            score += NAME_PHRASE_WEIGHT;
            break;
        }
    }
    // A query word that is a distinctive word of the style's own name or ref
    // (`bento`, `polaris`, `fluent`) is as strong as a keyword hit: it names
    // the style, and a single stray keyword of another style must not win.
    let name_words: Vec<String> = significant_words(&record.name.to_lowercase())
        .into_iter()
        .chain(significant_words(&record.reference.replace('-', " ")))
        .filter(|word| !STYLE_GENERIC_WORDS.contains(&word.as_str()))
        .collect();
    score += KEYWORD_WEIGHT * count_shared(words, &name_words);
    let keyword_words: Vec<String> = record
        .keywords
        .iter()
        .filter(|keyword| keyword.is_ascii() && keyword.contains([' ', '-']))
        .flat_map(|keyword| significant_words(keyword))
        .collect();
    score + KEYWORD_WORD_WEIGHT * count_shared(words, &keyword_words)
}

/// The JSON for one style: everything a designer needs to apply it, minus the
/// search-only keywords. Absent fields are left out, never empty.
fn style_json(hit: &StyleHit) -> Value {
    let record = hit.record;
    let mut object = Map::new();
    object.insert("id".into(), json!(record.id));
    object.insert("ref".into(), json!(record.reference));
    object.insert("name".into(), json!(record.name));
    for (key, value) in [
        ("category", &record.category),
        ("bestFor", &record.best_for),
        ("doNotUseFor", &record.do_not_use_for),
        ("effects", &record.effects),
        ("era", &record.era),
        ("complexity", &record.complexity),
        ("accessibility", &record.accessibility),
        ("lightDarkMode", &record.light_dark_mode),
        ("status", &record.status),
    ] {
        if let Some(text) = value {
            object.insert(key.into(), json!(text));
        }
    }
    for (key, colors) in [
        ("primaryColors", &record.primary_colors),
        ("secondaryColors", &record.secondary_colors),
    ] {
        if !colors.is_empty() {
            object.insert(key.into(), json!(colors));
        }
    }
    object.insert(
        "source".into(),
        json!({
            "name": record.source.name,
            "url": record.source.url,
            "commit": record.source.commit,
            "licenseSpdx": record.source.license_spdx,
            "retrieved": record.source.retrieved,
        }),
    );
    object.insert("score".into(), json!(hit.score));
    Value::Object(object)
}

fn no_match_message() -> String {
    json!({
        "matches": [],
        "message": "No close style matched; name a style from a product match or describe the look yourself.",
    })
    .to_string()
}

/// Fields dropped, in this order, from a single style that alone exceeds the cap.
const TRIM_ORDER: [&str; 6] = [
    "effects",
    "accessibility",
    "secondaryColors",
    "primaryColors",
    "era",
    "doNotUseFor",
];

/// Serialise `hits` as the tool result. Whole matches are dropped from the end
/// until the text fits [`MAX_RESPONSE_BYTES`]; a text is never sliced, so the
/// result is always valid JSON. If the best match alone is too large
/// (multi-byte text at the field limits) its optional fields are removed one
/// by one, least useful first.
pub fn styles_to_json(hits: &[StyleHit]) -> String {
    let mut matches: Vec<Value> = Vec::new();
    for hit in hits {
        let mut value = style_json(hit);
        let mut candidate = matches.clone();
        candidate.push(value.clone());
        if matches.is_empty() {
            for key in TRIM_ORDER {
                if envelope(&candidate).len() <= MAX_RESPONSE_BYTES {
                    break;
                }
                if let Value::Object(object) = &mut value {
                    object.remove(key);
                }
                candidate = vec![value.clone()];
            }
        }
        if envelope(&candidate).len() > MAX_RESPONSE_BYTES {
            break;
        }
        matches = candidate;
    }
    if matches.is_empty() {
        return no_match_message();
    }
    envelope(&matches)
}

/// Search and serialise in one step. `limit` is clamped to `1..=`[`MAX_LIMIT`].
pub fn style_data_json(query: &str, limit: usize) -> String {
    styles_to_json(&search_styles(query, limit.clamp(1, MAX_LIMIT)))
}

/// At most `max` whole comma-separated items of `text` that fit `max_chars`
/// characters; when even the first item is longer, cut it at a word boundary.
pub(crate) fn short_list(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for item in text.split(", ") {
        let joined = if out.is_empty() {
            item.to_string()
        } else {
            format!("{out}, {item}")
        };
        if joined.chars().count() > max_chars {
            break;
        }
        out = joined;
    }
    if !out.is_empty() {
        return out;
    }
    let cut: String = text.chars().take(max_chars).collect();
    match cut.rfind(' ') {
        Some(space) if text.chars().nth(max_chars).is_some_and(|c| c != ' ') => cut[..space]
            .trim_end_matches([',', ';', ':', '-'])
            .to_string(),
        _ => cut.trim_end().to_string(),
    }
}

/// `{ref, name, bestFor, doNotUseFor}` for the first [`MAX_SUMMARIES`] entries
/// of `styles` that are `external:` names with a record in `table`; names
/// without a record are skipped. The two texts are whole list items, at most
/// [`MAX_SUMMARY_CHARS`] characters.
pub(crate) fn summaries_from(table: &[StyleRecord], styles: &[String]) -> Vec<Value> {
    styles
        .iter()
        .filter(|style| style.starts_with(EXTERNAL_STYLE_PREFIX))
        .filter_map(|style| find_style(table, style))
        .take(MAX_SUMMARIES)
        .map(|record| {
            let mut object = Map::new();
            object.insert("ref".into(), json!(record.reference));
            object.insert("name".into(), json!(record.name));
            if let Some(text) = &record.best_for {
                object.insert("bestFor".into(), json!(short_list(text, MAX_SUMMARY_CHARS)));
            }
            if let Some(text) = &record.do_not_use_for {
                object.insert(
                    "doNotUseFor".into(),
                    json!(short_list(text, MAX_SUMMARY_CHARS)),
                );
            }
            Value::Object(object)
        })
        .collect()
}

/// [`summaries_from`] over the embedded table.
pub fn style_summaries(styles: &[String]) -> Vec<Value> {
    summaries_from(style_records(), styles)
}

#[cfg(test)]
#[path = "design_styles_search_tests.rs"]
mod tests;
