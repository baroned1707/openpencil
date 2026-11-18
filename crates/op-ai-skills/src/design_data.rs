//! Embedded design-data table: one record per PRODUCT TYPE (palette,
//! typography, layout pattern, decision rules, anti-patterns) that an agent
//! can look up through `get_guidelines { category: "data", query }`.
//!
//! This is *data, not prompt*: it is searched on demand (see
//! [`crate::design_data_search`]) and returned as TOOL RESULT content, never
//! placed in a system prompt. Because the table will later grow with imported
//! third-party datasets, the loader treats every string as untrusted: each
//! field has a length cap, control characters are rejected, and ONE malformed
//! record rejects the whole table (an empty table is safe; a half-trusted one
//! is not).
//!
//! The bytes are embedded on native targets only. The browser bundle has a
//! size ceiling and never answers `get_guidelines`, so on `wasm32` the same
//! API exists but the table is empty.

use std::fmt;
use std::sync::OnceLock;

use serde_json::{Map, Value};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const DESIGN_DATA_JSON: &str = include_str!("../data/design-data.json");
#[cfg(target_arch = "wasm32")]
pub(crate) const DESIGN_DATA_JSON: &str = "[]";

/// Hard ceiling on the raw JSON: a larger file is rejected unparsed.
pub const MAX_DATA_BYTES: usize = 2 * 1024 * 1024;
/// Hard ceiling on the number of records.
pub const MAX_RECORDS: usize = 1000;

pub const MAX_ID_CHARS: usize = 48;
pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_KEYWORD_CHARS: usize = 40;
pub const MAX_KEYWORDS: usize = 40;
pub const MAX_PATTERN_CHARS: usize = 400;
pub const MAX_STYLE_CHARS: usize = 50;
pub const MAX_STYLES: usize = 6;
/// `when` plus `do` of one rule together.
pub const MAX_RULE_CHARS: usize = 300;
pub const MAX_RULES: usize = 6;
pub const MAX_AVOID_CHARS: usize = 160;
pub const MAX_AVOID: usize = 5;
pub const MAX_NOTES_CHARS: usize = 600;
const MAX_FONT_CHARS: usize = 80;
const MAX_MOOD_CHARS: usize = 120;
const MAX_SOURCE_CHARS: usize = 120;

/// Platforms a record can target.
pub const PLATFORMS: [&str; 5] = ["web", "mobile", "desktop", "slides", "card"];
/// A style name that is deliberately not one of the embedded style guides
/// carries this prefix (the guide test skips it).
pub const EXTERNAL_STYLE_PREFIX: &str = "external:";

/// Colour roles of a record, all `#rrggbb` lower-case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignPalette {
    pub primary: String,
    pub on_primary: String,
    pub secondary: String,
    pub accent: String,
    pub background: String,
    pub foreground: String,
    pub card: String,
    pub muted: String,
    pub border: String,
    pub destructive: String,
}

impl DesignPalette {
    /// `(json key, value)` in the order the roles are documented.
    pub fn entries(&self) -> [(&'static str, &str); 10] {
        [
            ("primary", &self.primary),
            ("onPrimary", &self.on_primary),
            ("secondary", &self.secondary),
            ("accent", &self.accent),
            ("background", &self.background),
            ("foreground", &self.foreground),
            ("card", &self.card),
            ("muted", &self.muted),
            ("border", &self.border),
            ("destructive", &self.destructive),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignTypography {
    pub heading: String,
    pub body: String,
    pub mood: String,
}

/// One decision rule: in the situation `when`, do `action` (JSON key `do`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignRule {
    pub when: String,
    pub action: String,
}

/// Where a record came from. Required on every record so a later import
/// cannot ship without attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignSource {
    pub name: String,
    pub url: String,
    pub commit: String,
    pub license_spdx: String,
    /// `YYYY-MM-DD`.
    pub retrieved: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignRecord {
    pub id: String,
    pub name: String,
    pub keywords: Vec<String>,
    pub platforms: Vec<String>,
    pub pattern: String,
    pub styles: Vec<String>,
    pub palette: DesignPalette,
    pub typography: DesignTypography,
    pub rules: Vec<DesignRule>,
    pub avoid: Vec<String>,
    pub source: DesignSource,
    pub notes: String,
}

/// Why one field was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProblem {
    Missing,
    WrongType,
    Empty,
    TooLong { max: usize },
    TooMany { max: usize },
    ControlCharacter,
    BadFormat(&'static str),
    DuplicateId,
}

/// Why the whole table was refused.
#[derive(Debug)]
pub enum DesignDataError {
    TooLarge {
        bytes: usize,
    },
    Json(serde_json::Error),
    NotAnArray,
    TooManyRecords {
        count: usize,
    },
    Record {
        /// Record id when it could be read, else `#<index>`.
        record: String,
        field: String,
        problem: FieldProblem,
    },
}

impl fmt::Display for FieldProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("is missing"),
            Self::WrongType => f.write_str("has the wrong JSON type"),
            Self::Empty => f.write_str("is empty"),
            Self::TooLong { max } => write!(f, "is longer than {max} characters"),
            Self::TooMany { max } => write!(f, "has more than {max} entries"),
            Self::ControlCharacter => f.write_str("contains a control character"),
            Self::BadFormat(expected) => write!(f, "must be {expected}"),
            Self::DuplicateId => f.write_str("duplicates an earlier record"),
        }
    }
}

impl fmt::Display for DesignDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => {
                write!(f, "design data is {bytes} bytes, limit {MAX_DATA_BYTES}")
            }
            Self::Json(error) => write!(f, "design data is not valid JSON: {error}"),
            Self::NotAnArray => f.write_str("design data must be a JSON array"),
            Self::TooManyRecords { count } => {
                write!(f, "design data has {count} records, limit {MAX_RECORDS}")
            }
            Self::Record {
                record,
                field,
                problem,
            } => write!(f, "record {record}: field {field} {problem}"),
        }
    }
}

impl std::error::Error for DesignDataError {}

/// Parse and validate a design-data JSON document. Pure: the embed and the
/// process-wide cache sit on top of it so tests can feed any string.
pub fn parse_design_data(raw: &str) -> Result<Vec<DesignRecord>, DesignDataError> {
    if raw.len() > MAX_DATA_BYTES {
        return Err(DesignDataError::TooLarge { bytes: raw.len() });
    }
    let root: Value = serde_json::from_str(raw).map_err(DesignDataError::Json)?;
    let Value::Array(items) = root else {
        return Err(DesignDataError::NotAnArray);
    };
    if items.len() > MAX_RECORDS {
        return Err(DesignDataError::TooManyRecords { count: items.len() });
    }
    let mut records: Vec<DesignRecord> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let record = parse_record(index, item)?;
        if records.iter().any(|earlier| earlier.id == record.id) {
            return Err(problem(&record.id, "id", FieldProblem::DuplicateId));
        }
        records.push(record);
    }
    Ok(records)
}

/// Every validated record, parsed once. A bad embedded table yields an empty
/// slice and one `eprintln`; it never panics and never blocks start-up.
pub fn design_records() -> &'static [DesignRecord] {
    static CACHE: OnceLock<Vec<DesignRecord>> = OnceLock::new();
    CACHE.get_or_init(|| match parse_design_data(DESIGN_DATA_JSON) {
        Ok(records) => records,
        Err(error) => {
            eprintln!("op-ai-skills: design data disabled: {error}");
            Vec::new()
        }
    })
}

fn problem(record: &str, field: &str, problem: FieldProblem) -> DesignDataError {
    DesignDataError::Record {
        record: record.to_string(),
        field: field.to_string(),
        problem,
    }
}

/// Field reader bound to one record, so every error names record and field.
struct Reader<'a> {
    label: String,
    object: &'a Map<String, Value>,
}

impl<'a> Reader<'a> {
    fn fail(&self, field: &str, why: FieldProblem) -> DesignDataError {
        problem(&self.label, field, why)
    }

    fn value(&self, field: &str) -> Result<&'a Value, DesignDataError> {
        self.object
            .get(field)
            .ok_or_else(|| self.fail(field, FieldProblem::Missing))
    }

    fn text(&self, field: &str, max: usize) -> Result<String, DesignDataError> {
        let Value::String(text) = self.value(field)? else {
            return Err(self.fail(field, FieldProblem::WrongType));
        };
        self.check_text(field, text, max)?;
        Ok(text.clone())
    }

    fn check_text(&self, field: &str, text: &str, max: usize) -> Result<(), DesignDataError> {
        if text.trim().is_empty() {
            return Err(self.fail(field, FieldProblem::Empty));
        }
        if text.chars().count() > max {
            return Err(self.fail(field, FieldProblem::TooLong { max }));
        }
        if text.chars().any(|c| c.is_control() && c != '\n') {
            return Err(self.fail(field, FieldProblem::ControlCharacter));
        }
        Ok(())
    }

    fn list(
        &self,
        field: &str,
        max_entries: usize,
        required: bool,
    ) -> Result<&'a [Value], DesignDataError> {
        let Value::Array(items) = self.value(field)? else {
            return Err(self.fail(field, FieldProblem::WrongType));
        };
        if required && items.is_empty() {
            return Err(self.fail(field, FieldProblem::Empty));
        }
        if items.len() > max_entries {
            return Err(self.fail(field, FieldProblem::TooMany { max: max_entries }));
        }
        Ok(items)
    }

    fn texts(
        &self,
        field: &str,
        max_entries: usize,
        max_chars: usize,
        required: bool,
    ) -> Result<Vec<String>, DesignDataError> {
        let mut out = Vec::new();
        for item in self.list(field, max_entries, required)? {
            let Value::String(text) = item else {
                return Err(self.fail(field, FieldProblem::WrongType));
            };
            self.check_text(field, text, max_chars)?;
            out.push(text.clone());
        }
        Ok(out)
    }

    fn object(&self, field: &str) -> Result<Reader<'a>, DesignDataError> {
        let Value::Object(object) = self.value(field)? else {
            return Err(self.fail(field, FieldProblem::WrongType));
        };
        Ok(Reader {
            label: self.label.clone(),
            object,
        })
    }
}

fn parse_record(index: usize, item: &Value) -> Result<DesignRecord, DesignDataError> {
    let placeholder = format!("#{index}");
    let Value::Object(object) = item else {
        return Err(problem(&placeholder, "(record)", FieldProblem::WrongType));
    };
    let mut reader = Reader {
        label: placeholder,
        object,
    };
    let id = reader.text("id", MAX_ID_CHARS)?;
    if !is_slug(&id) {
        return Err(reader.fail("id", FieldProblem::BadFormat("a slug of a-z, 0-9 and -")));
    }
    reader.label = id.clone();

    let keywords = reader.texts("keywords", MAX_KEYWORDS, MAX_KEYWORD_CHARS, true)?;
    for keyword in &keywords {
        if keyword.contains('\n')
            || keyword.trim() != keyword
            || keyword.chars().any(|c| c.is_uppercase())
        {
            return Err(reader.fail(
                "keywords",
                FieldProblem::BadFormat("trimmed, single-line and lower-case"),
            ));
        }
    }
    let platforms = reader.texts("platforms", PLATFORMS.len(), 16, true)?;
    if platforms.iter().any(|p| !PLATFORMS.contains(&p.as_str())) {
        return Err(reader.fail(
            "platforms",
            FieldProblem::BadFormat("one of web, mobile, desktop, slides, card"),
        ));
    }
    let styles = reader.texts("styles", MAX_STYLES, MAX_STYLE_CHARS, false)?;

    let mut rules = Vec::new();
    for rule in reader.list("rules", MAX_RULES, true)? {
        let Value::Object(object) = rule else {
            return Err(reader.fail("rules", FieldProblem::WrongType));
        };
        let rule_reader = Reader {
            label: reader.label.clone(),
            object,
        };
        let when = rule_reader.text("when", MAX_RULE_CHARS)?;
        let action = rule_reader.text("do", MAX_RULE_CHARS)?;
        if when.chars().count() + action.chars().count() > MAX_RULE_CHARS {
            return Err(reader.fail(
                "rules",
                FieldProblem::TooLong {
                    max: MAX_RULE_CHARS,
                },
            ));
        }
        rules.push(DesignRule { when, action });
    }

    let palette = parse_palette(&reader.object("palette")?)?;
    let typography_reader = reader.object("typography")?;
    let typography = DesignTypography {
        heading: typography_reader.text("heading", MAX_FONT_CHARS)?,
        body: typography_reader.text("body", MAX_FONT_CHARS)?,
        mood: typography_reader.text("mood", MAX_MOOD_CHARS)?,
    };
    let source = parse_source(&reader.object("source")?)?;

    Ok(DesignRecord {
        name: reader.text("name", MAX_NAME_CHARS)?,
        keywords,
        platforms,
        pattern: reader.text("pattern", MAX_PATTERN_CHARS)?,
        styles,
        palette,
        typography,
        rules,
        avoid: reader.texts("avoid", MAX_AVOID, MAX_AVOID_CHARS, false)?,
        source,
        notes: reader.text("notes", MAX_NOTES_CHARS)?,
        id,
    })
}

fn parse_palette(reader: &Reader<'_>) -> Result<DesignPalette, DesignDataError> {
    let color = |field: &str| -> Result<String, DesignDataError> {
        let text = reader.text(field, 7)?;
        if !is_hex_color(&text) {
            return Err(reader.fail(field, FieldProblem::BadFormat("a #rrggbb colour")));
        }
        Ok(text.to_ascii_lowercase())
    };
    Ok(DesignPalette {
        primary: color("primary")?,
        on_primary: color("onPrimary")?,
        secondary: color("secondary")?,
        accent: color("accent")?,
        background: color("background")?,
        foreground: color("foreground")?,
        card: color("card")?,
        muted: color("muted")?,
        border: color("border")?,
        destructive: color("destructive")?,
    })
}

fn parse_source(reader: &Reader<'_>) -> Result<DesignSource, DesignDataError> {
    let url = reader.text("url", MAX_SOURCE_CHARS)?;
    if !url.starts_with("https://") || url.chars().any(char::is_whitespace) {
        return Err(reader.fail("source.url", FieldProblem::BadFormat("an https:// URL")));
    }
    let license_spdx = reader.text("licenseSpdx", 60)?;
    if !license_spdx
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || ".-+() ".contains(c))
    {
        return Err(reader.fail(
            "source.licenseSpdx",
            FieldProblem::BadFormat("an SPDX licence expression"),
        ));
    }
    let retrieved = reader.text("retrieved", 10)?;
    if !is_iso_date(&retrieved) {
        return Err(reader.fail("source.retrieved", FieldProblem::BadFormat("YYYY-MM-DD")));
    }
    Ok(DesignSource {
        name: reader.text("name", MAX_NAME_CHARS)?,
        url,
        commit: reader.text("commit", 64)?,
        license_spdx,
        retrieved,
    })
}

/// `^[a-z0-9-]+$`
fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `^#[0-9a-fA-F]{6}$` (strict: no short form, no alpha).
fn is_hex_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// `YYYY-MM-DD` with a plausible month and day.
fn is_iso_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !text.is_ascii() || bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| text[range].bytes().all(|b| b.is_ascii_digit());
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let month: u32 = text[5..7].parse().unwrap_or(0);
    let day: u32 = text[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

#[cfg(test)]
#[path = "design_data_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "design_data_notices_tests.rs"]
mod notices_tests;
