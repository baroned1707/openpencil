//! Fontsource catalog (`GET https://api.fontsource.org/v1/fonts`): parsing and
//! the exact-name lookup that decides whether a family may be fetched at all.
//!
//! Document `font-family` strings are untrusted (they also arrive through
//! `.fig` imports), so the family string is only ever used as a lookup key
//! into this catalog. Nothing downstream builds a path or URL from it.

use std::collections::HashMap;

use serde::Deserialize;

use crate::license::License;

pub const CATALOG_URL: &str = "https://api.fontsource.org/v1/fonts";
/// The real catalog is ~540 KB; anything near this cap is not the catalog.
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const MAX_FAMILY_CHARS: usize = 100;
const MAX_ID_CHARS: usize = 80;

/// Normalise a document family name for lookup: trim, strip one pair of
/// surrounding quotes, collapse runs of whitespace, lowercase. Returns `None`
/// for an empty name, a control character (including NUL) or an absurd length.
pub fn normalize_family(raw: &str) -> Option<String> {
    let mut text = raw.trim();
    for quote in ['"', '\''] {
        if text.len() >= 2 && text.starts_with(quote) && text.ends_with(quote) {
            text = text[1..text.len() - 1].trim();
            break;
        }
    }
    if text.is_empty() || text.chars().count() > MAX_FAMILY_CHARS {
        return None;
    }
    if text.chars().any(char::is_control) {
        return None;
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(collapsed.to_lowercase())
}

/// `^[a-z0-9-]+$`, bounded. The catalog `id` is the on-disk cache key.
pub fn is_valid_catalog_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// One Google family from the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// Validated `^[a-z0-9-]+$` token.
    pub id: String,
    pub family: String,
    /// Raw Fontsource licence string, kept for diagnostics.
    pub license_raw: String,
    /// `None` when the licence is not on the allow-list.
    pub license: Option<License>,
}

#[derive(Debug, Deserialize)]
struct RawEntry {
    id: Option<String>,
    family: Option<String>,
    license: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

/// Parsed catalog with a normalised-family index.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
    by_family: HashMap<String, usize>,
}

impl Catalog {
    /// Parse the Fontsource array. Malformed JSON is an error; a malformed or
    /// hostile ENTRY (bad id, empty family, non-Google `type`) is skipped so
    /// one bad row cannot poison the rest.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let raw: Vec<serde_json::Value> =
            serde_json::from_slice(bytes).map_err(|error| format!("catalog JSON: {error}"))?;
        let mut catalog = Self::default();
        for value in raw {
            let Ok(entry) = serde_json::from_value::<RawEntry>(value) else {
                continue;
            };
            if entry.kind.as_deref() != Some("google") {
                continue;
            }
            let (Some(id), Some(family)) = (entry.id, entry.family) else {
                continue;
            };
            if !is_valid_catalog_id(&id) {
                continue;
            }
            let Some(key) = normalize_family(&family) else {
                continue;
            };
            let license_raw = entry.license.unwrap_or_default();
            let license = License::from_catalog(&license_raw);
            if catalog.by_family.contains_key(&key) {
                continue;
            }
            catalog.by_family.insert(key, catalog.entries.len());
            catalog.entries.push(CatalogEntry {
                id,
                family: family.trim().to_string(),
                license_raw,
                license,
            });
        }
        Ok(catalog)
    }

    /// Exact lookup on the normalised family name. No prefix, fuzzy or
    /// alias matching: an unknown or proprietary name is simply absent.
    pub fn lookup(&self, family: &str) -> Option<&CatalogEntry> {
        let key = normalize_family(family)?;
        self.by_family.get(&key).map(|&index| &self.entries[index])
    }

    /// Every family name as the catalog spells it, in catalog order.
    pub fn families(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.family.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
