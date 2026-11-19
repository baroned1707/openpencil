//! Glyph-name validation for `icon_font` nodes.
//!
//! A misspelled glyph name paints as nothing, and nothing in the document says
//! so. This module answers "does the canvas know this glyph?" with the SAME
//! lookup the painter uses (`icon_catalog::lookup_icon`, which already applies
//! the legacy-name aliases and the feather fallback, plus the hand-drawn
//! `Icon::from_name` set for lucide), so a name that paints is never flagged.
//! Only the embedded lucide / feather sets are checked; any other family
//! (`simple-icons` is a runtime-loaded brand catalog) is skipped.

use op_editor_ui::widgets::icon_catalog::{core_catalog_loaded, core_icon_names, lookup_icon};
use op_editor_ui::widgets::icons::Icon;

/// How many nearest glyph names an issue line offers.
const SUGGESTION_COUNT: usize = 3;

/// A near-match (prefix, or a whole hyphen segment) is never ranked worse than
/// this edit distance: `cart` should offer `shopping-cart` before unrelated
/// four-letter names.
const RELATED_DISTANCE: usize = 2;

/// The embedded collection an `icon_font` family resolves to, or `None` when
/// the family is not one whose glyph set can be checked.
fn checkable_collection(family: Option<&str>) -> Option<&'static str> {
    match family.map(str::trim).unwrap_or("") {
        "" | "lucide" => Some("lucide"),
        "feather" => Some("feather"),
        _ => None,
    }
}

/// Whether the painter draws `name` for `collection` (the catalog with its
/// legacy-name aliases, then the chrome table for lucide).
fn painted_in(collection: &str, name: &str) -> bool {
    lookup_icon(collection, name).is_some()
        || (collection == "lucide" && Icon::from_name(name.trim()).is_some())
}

/// `true` when the painter would draw `name`, or when the family or catalog
/// cannot be judged (so nothing may be changed on that basis).
pub(super) fn painter_knows_glyph(family: Option<&str>, name: &str) -> bool {
    match checkable_collection(family) {
        Some(collection) if core_catalog_loaded() => painted_in(collection, name),
        _ => true,
    }
}

/// `Some(issue line)` when `name` is not a glyph the canvas can paint for
/// `family`; `None` for a known glyph, an unchecked family or a catalog that
/// is not loaded.
pub(super) fn unknown_glyph_issue(id: &str, family: Option<&str>, name: &str) -> Option<String> {
    let collection = checkable_collection(family)?;
    if !core_catalog_loaded() {
        return None;
    }
    if painted_in(collection, name) {
        return None;
    }
    let closest = nearest_glyph_names(collection, name.trim());
    let mut line = format!("icon {id}: '{name}' is not a {collection} glyph");
    if !closest.is_empty() {
        line.push_str("; closest: ");
        line.push_str(&closest.join(", "));
    }
    Some(line)
}

/// Shortest name a typo fix is trusted for: one edit on a three-letter name
/// reaches too many other glyphs to call it what was meant.
const MIN_RENAME_LEN: usize = 4;

/// The glyph name to write instead of the unpainted `name`, when exactly one is
/// clearly meant: `name` normalised (case, `_` and spaces to `-`) is a name the
/// painter draws, or it is one edit from exactly one glyph of the family's
/// set. `None` when the family cannot be judged, the name is a family name or
/// a variable reference, too short, ambiguous or unmatched.
pub(super) fn rename_target(family: Option<&str>, name: &str) -> Option<String> {
    let collection = checkable_collection(family)?;
    if !core_catalog_loaded() {
        return None;
    }
    let trimmed = name.trim();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with('$') || lowered.starts_with("lucide") || lowered.starts_with("material")
    {
        return None;
    }
    let normalised: String = lowered
        .chars()
        .map(|c| {
            if c == '_' || c.is_whitespace() {
                '-'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if normalised.chars().count() < MIN_RENAME_LEN {
        return None;
    }
    if normalised != trimmed && painted_in(collection, &normalised) {
        return Some(normalised);
    }
    // Compare against the raw edit distance, not the picker's ranking
    // distance, which treats a prefix as close.
    let mut targets: Vec<&'static str> = core_icon_names(collection)
        .filter(|candidate| edit_distance(&normalised, candidate) == 1)
        .collect();
    targets.sort_unstable();
    targets.dedup();
    match targets.as_slice() {
        [only] => Some((*only).to_string()),
        _ => None,
    }
}

/// Up to [`SUGGESTION_COUNT`] glyph names nearest to `name`, best first.
/// Deterministic: ties break on the shorter name, then alphabetically.
fn nearest_glyph_names(collection: &str, name: &str) -> Vec<&'static str> {
    let query = name.to_ascii_lowercase();
    let mut ranked: Vec<(usize, usize, &'static str)> = core_icon_names(collection)
        .map(|candidate| {
            (
                glyph_distance(&query, candidate),
                candidate.len(),
                candidate,
            )
        })
        .collect();
    ranked.sort_unstable();
    ranked
        .into_iter()
        .take(SUGGESTION_COUNT)
        .map(|(_, _, candidate)| candidate)
        .collect()
}

fn glyph_distance(query: &str, candidate: &str) -> usize {
    let edit = edit_distance(query, candidate);
    let related = !query.is_empty()
        && (candidate.starts_with(query)
            || query.starts_with(candidate)
            || candidate.split('-').any(|segment| segment == query));
    if related {
        edit.min(RELATED_DISTANCE)
    } else {
        edit
    }
}

/// Levenshtein distance over chars (two-row dynamic programme).
pub(super) fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (row, ca) in a.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, cb) in b.iter().enumerate() {
            let substitution = previous[column] + usize::from(ca != *cb);
            let insertion = current[column] + 1;
            let deletion = previous[column + 1] + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        previous = current;
    }
    previous[b.len()]
}
