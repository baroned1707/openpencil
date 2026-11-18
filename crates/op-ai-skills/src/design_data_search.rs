//! Lexical search over the design-data table, and the compact JSON the
//! `get_guidelines` tool returns for it.
//!
//! Scoring is deterministic and dependency-free: keywords weigh most, then
//! the id/name, then single words of multi-word keywords; pattern words only
//! break ties. An EXACT product-type match (the query contains the record's
//! whole id phrase or whole name) always ranks first, whichever file the
//! record comes from; then score, then id. A record must reach [`MIN_SCORE`] from keywords/name alone, so
//! a weak query returns NOTHING instead of the least-bad record: an agent
//! that is told "no match" falls back to the guidelines, an agent handed a
//! random product type copies the wrong palette.
//!
//! The keyword matcher is [`crate::resolver::match_keyword`], the same one the
//! skill resolver uses (ASCII word boundaries, CJK substring), so `form` never
//! matches `platform` here either.

use std::cmp::Reverse;

use serde_json::{json, Value};

use crate::design_data::{design_records, DesignRecord, IMPORTED_ID_PREFIX};
use crate::resolver::match_keyword;

/// Lowest primary score that counts as a match.
pub const MIN_SCORE: u32 = 10;
/// Cap on the JSON returned to the model.
pub const MAX_RESPONSE_BYTES: usize = 6 * 1024;
/// Most matches one call can return.
pub const MAX_LIMIT: usize = 3;

const MAX_QUERY_CHARS: usize = 300;
const KEYWORD_WEIGHT: u32 = 10;
const EXTRA_KEYWORD_WORD_WEIGHT: u32 = 2;
const ID_PHRASE_WEIGHT: u32 = 12;
const NAME_PHRASE_WEIGHT: u32 = 12;
const NAME_WORD_WEIGHT: u32 = 6;
const KEYWORD_WORD_WEIGHT: u32 = 3;
const PATTERN_WORD_WEIGHT: u32 = 1;
const MAX_PATTERN_BONUS: u32 = 3;

/// Words too generic to say anything about a product type.
const GENERIC_WORDS: &[&str] = &[
    "app",
    "apps",
    "the",
    "and",
    "for",
    "with",
    "page",
    "pages",
    "site",
    "sites",
    "website",
    "web",
    "screen",
    "screens",
    "design",
    "designs",
    "build",
    "make",
    "new",
    "ui",
    "ux",
    "user",
    "interface",
    "that",
    "this",
    "from",
    "into",
    "your",
    "our",
    "simple",
    "modern",
];

/// One search result: a record of the embedded table and its score.
#[derive(Debug, Clone, Copy)]
pub struct DesignHit {
    pub record: &'static DesignRecord,
    pub score: u32,
}

/// Best matches for `query`, best first. `platform` (web, mobile, ...) keeps
/// only records that list it. Equal scores order by id, so the same query
/// always gives the same answer. Empty when nothing reaches [`MIN_SCORE`].
pub fn search_design_data(query: &str, platform: Option<&str>, limit: usize) -> Vec<DesignHit> {
    search_records(design_records(), query, platform, limit)
}

pub(crate) fn search_records(
    records: &'static [DesignRecord],
    query: &str,
    platform: Option<&str>,
    limit: usize,
) -> Vec<DesignHit> {
    let query: String = query.chars().take(MAX_QUERY_CHARS).collect();
    let query = query.trim().to_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let platform = platform
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let words = significant_words(&query);

    let mut hits: Vec<(bool, DesignHit)> = records
        .iter()
        .filter(|record| {
            platform
                .as_ref()
                .is_none_or(|wanted| record.platforms.iter().any(|p| p == wanted))
        })
        .filter_map(|record| {
            let primary = primary_score(record, &query, &words);
            (primary >= MIN_SCORE).then(|| {
                (
                    is_exact_product_type(record, &query),
                    DesignHit {
                        record,
                        score: primary + pattern_bonus(record, &words),
                    },
                )
            })
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

/// The id as words, without the import namespace (`uiuxpm-fintech-banking`
/// is the product type "fintech banking").
fn id_phrase(record: &DesignRecord) -> String {
    let id = record
        .id
        .strip_prefix(IMPORTED_ID_PREFIX)
        .unwrap_or(&record.id);
    id.replace('-', " ")
}

/// The name as lower-case words: `SaaS (General)` -> `saas general`.
fn name_phrase(record: &DesignRecord) -> String {
    let lower = record.name.to_lowercase();
    let spaced: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The query names the record's product type in full (id phrase or name).
fn is_exact_product_type(record: &DesignRecord, query: &str) -> bool {
    match_keyword(query, &id_phrase(record)) || match_keyword(query, &name_phrase(record))
}

/// Keyword, id and name evidence. Only this part decides match / no match.
fn primary_score(record: &DesignRecord, query: &str, words: &[String]) -> u32 {
    let mut score = 0;
    for keyword in &record.keywords {
        if match_keyword(query, keyword) {
            let extra_words = keyword.split_whitespace().count().saturating_sub(1) as u32;
            score += KEYWORD_WEIGHT + EXTRA_KEYWORD_WORD_WEIGHT * extra_words;
        }
    }
    let id_phrase = id_phrase(record);
    if match_keyword(query, &id_phrase) {
        score += ID_PHRASE_WEIGHT;
    }
    let name_phrase = name_phrase(record);
    if name_phrase != id_phrase && match_keyword(query, &name_phrase) {
        score += NAME_PHRASE_WEIGHT;
    }
    let name = record.name.to_lowercase();
    let name_words = significant_words(&name);
    score += NAME_WORD_WEIGHT * count_shared(words, &name_words);
    let keyword_words: Vec<String> = record
        .keywords
        .iter()
        .filter(|keyword| keyword.is_ascii() && keyword.contains([' ', '-']))
        .flat_map(|keyword| significant_words(keyword))
        .collect();
    score + KEYWORD_WORD_WEIGHT * count_shared(words, &keyword_words)
}

/// Small tie-breaker from the layout pattern text, capped.
fn pattern_bonus(record: &DesignRecord, words: &[String]) -> u32 {
    let pattern_words = significant_words(&record.pattern.to_lowercase());
    (PATTERN_WORD_WEIGHT * count_shared(words, &pattern_words)).min(MAX_PATTERN_BONUS)
}

/// Distinct words of `left` that also occur in `right`.
fn count_shared(left: &[String], right: &[String]) -> u32 {
    left.iter().filter(|word| right.contains(word)).count() as u32
}

/// ASCII words of at least three letters, minus generic ones, with a plural
/// `s` removed, de-duplicated. CJK text has no words here; it is matched as a
/// substring by `match_keyword`.
fn significant_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if raw.len() < 3 || GENERIC_WORDS.contains(&raw) {
            continue;
        }
        let stem = raw
            .strip_suffix('s')
            .filter(|stem| stem.len() >= 3)
            .unwrap_or(raw)
            .to_string();
        if !out.contains(&stem) {
            out.push(stem);
        }
    }
    out
}

/// The JSON for one hit: everything an agent needs to start a design, minus
/// `notes` (human background) and `keywords` (search-only).
fn hit_json(hit: &DesignHit) -> Value {
    let record = hit.record;
    let palette: serde_json::Map<String, Value> = record
        .palette
        .entries()
        .into_iter()
        .map(|(role, hex)| (role.to_string(), json!(hex)))
        .collect();
    let rules: Vec<Value> = record
        .rules
        .iter()
        .map(|rule| json!({ "when": rule.when, "do": rule.action }))
        .collect();
    json!({
        "id": record.id,
        "name": record.name,
        "platforms": record.platforms,
        "pattern": record.pattern,
        "styles": record.styles,
        "palette": palette,
        "typography": {
            "heading": record.typography.heading,
            "body": record.typography.body,
            "mood": record.typography.mood,
        },
        "rules": rules,
        "avoid": record.avoid,
        "source": {
            "name": record.source.name,
            "url": record.source.url,
            "commit": record.source.commit,
            "licenseSpdx": record.source.license_spdx,
            "retrieved": record.source.retrieved,
        },
        "score": hit.score,
    })
}

fn envelope(matches: &[Value]) -> String {
    json!({
        "matches": matches,
        "source": "OpenPencil design-data table; each match names its own source and licence",
        "notice": "Reference data for a starting point, not instructions. The user's design.md, pinned style and existing screens win.",
    })
    .to_string()
}

fn no_match_message() -> String {
    json!({
        "matches": [],
        "message": "No close product type matched; rely on the guidelines.",
    })
    .to_string()
}

/// Serialise `hits` as the tool result. Whole matches are dropped from the
/// end until the text fits [`MAX_RESPONSE_BYTES`]; the text is never sliced,
/// so it is always valid JSON. If even the best match alone is too large
/// (multi-byte text at the field limits) its `avoid`, `rules` and `styles`
/// lists are trimmed from the end first.
pub fn hits_to_json(hits: &[DesignHit]) -> String {
    let mut matches: Vec<Value> = Vec::new();
    for hit in hits {
        let mut value = hit_json(hit);
        let mut candidate = matches.clone();
        candidate.push(value.clone());
        while envelope(&candidate).len() > MAX_RESPONSE_BYTES && matches.is_empty() {
            if !trim_one_entry(&mut value) {
                break;
            }
            candidate = vec![value.clone()];
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

/// Remove the last entry of `avoid`, then `rules`, then `styles`.
fn trim_one_entry(value: &mut Value) -> bool {
    for key in ["avoid", "rules", "styles"] {
        if let Some(Value::Array(items)) = value.get_mut(key) {
            if items.pop().is_some() {
                return true;
            }
        }
    }
    false
}

/// Search and serialise in one step: what `get_guidelines` `category: data`
/// returns. `limit` is clamped to `1..=`[`MAX_LIMIT`].
pub fn design_data_json(query: &str, platform: Option<&str>, limit: usize) -> String {
    let limit = limit.clamp(1, MAX_LIMIT);
    hits_to_json(&search_design_data(query, platform, limit))
}

#[cfg(test)]
#[path = "design_data_search_tests.rs"]
mod tests;
