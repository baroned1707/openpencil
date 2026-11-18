//! The quick switcher's fuzzy matcher: pure, allocation-light, no state.
//!
//! One question: how well does `query` match `text`? The answer is a
//! [`MatchScore`] that orders matches, best first:
//!
//! 1. [`MatchTier::Exact`]: the whole name (the `.op` extension aside).
//! 2. [`MatchTier::Prefix`]: the name starts with the query.
//! 3. [`MatchTier::WordStart`]: the query starts at the beginning of a word
//!    inside the name (after `-`, `_`, space, `.`, `/`, or at a lower-to-upper
//!    camelCase step), so `card` finds `product-card`.
//! 4. [`MatchTier::Subsequence`]: the query's letters appear in order; the
//!    tighter the letters sit, the better (`chk` finds `checkout`).
//!
//! A name that does not match can still be listed when the query matches its
//! PROJECT's name ([`score_in_project`], [`MatchTier::Project`], last): typing
//! `shop` lists the files of the `shop` project.
//!
//! Matching ignores case and whitespace in the query. Case folding maps one
//! character to one character, so indices stay aligned with the text.

/// How a query matched, best first. The order of the variants IS the ranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchTier {
    Exact,
    Prefix,
    WordStart,
    Subsequence,
    /// Only the project's name matched.
    Project,
}

/// A match's rank: a lower score is a better match, so `sort` puts the best
/// first. Equal scores keep their input order when sorted with a stable sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MatchScore {
    pub tier: MatchTier,
    /// Extra distance between the matched letters (always `0` above
    /// [`MatchTier::Subsequence`]).
    pub gaps: usize,
}

fn fold(character: char) -> char {
    character.to_lowercase().next().unwrap_or(character)
}

/// The query without whitespace and without a typed `.op`, case folded.
fn normalized_query(query: &str) -> Vec<char> {
    let mut wanted: Vec<char> = query
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(fold)
        .collect();
    if wanted.len() > 3 && wanted.ends_with(&['.', 'o', 'p']) {
        wanted.truncate(wanted.len() - 3);
    }
    wanted
}

/// The name without a trailing `.op`, so `op` is not a match for every file.
fn stem(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() > 3 && bytes[bytes.len() - 3..].eq_ignore_ascii_case(b".op") {
        &text[..text.len() - 3]
    } else {
        text
    }
}

/// Whether a word begins at `index` of `text`.
fn word_starts_at(text: &[char], index: usize) -> bool {
    match index.checked_sub(1).map(|before| text[before]) {
        None => true,
        Some(before) => {
            !before.is_alphanumeric() || (before.is_lowercase() && text[index].is_uppercase())
        }
    }
}

/// Score `query` against `text`, or `None` when it does not match. An empty
/// query matches everything with the best-possible-but-equal score, so a
/// stable sort keeps the input order.
pub fn score(query: &str, text: &str) -> Option<MatchScore> {
    let wanted = normalized_query(query);
    if wanted.is_empty() {
        return Some(MatchScore {
            tier: MatchTier::Exact,
            gaps: 0,
        });
    }
    let original: Vec<char> = stem(text).chars().collect();
    let folded: Vec<char> = original.iter().copied().map(fold).collect();
    if folded == wanted {
        return Some(MatchScore {
            tier: MatchTier::Exact,
            gaps: 0,
        });
    }
    if folded.starts_with(&wanted) {
        return Some(MatchScore {
            tier: MatchTier::Prefix,
            gaps: 0,
        });
    }
    let word_start = (1..folded.len())
        .any(|index| word_starts_at(&original, index) && folded[index..].starts_with(&wanted));
    if word_start {
        return Some(MatchScore {
            tier: MatchTier::WordStart,
            gaps: 0,
        });
    }
    let mut first = None;
    let mut last = 0;
    let mut wanted_iter = wanted.iter().peekable();
    for (index, character) in folded.iter().enumerate() {
        if wanted_iter.peek() == Some(&character) {
            wanted_iter.next();
            first.get_or_insert(index);
            last = index;
        }
    }
    if wanted_iter.peek().is_some() {
        return None;
    }
    Some(MatchScore {
        tier: MatchTier::Subsequence,
        gaps: (last + 1 - first.unwrap_or(0)).saturating_sub(wanted.len()),
    })
}

/// [`score`] of a file's `label`, falling back to a match on its `project`
/// name (ranked after every match on the label itself).
pub fn score_in_project(query: &str, label: &str, project: &str) -> Option<MatchScore> {
    score(query, label).or_else(|| {
        let named = !project.is_empty() && !query.trim().is_empty();
        named
            .then(|| score(query, project))
            .flatten()
            .map(|_| MatchScore {
                tier: MatchTier::Project,
                gaps: 0,
            })
    })
}

#[cfg(test)]
#[path = "quick_switcher_match_tests.rs"]
mod tests;
