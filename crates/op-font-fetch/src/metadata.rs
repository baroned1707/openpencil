//! Tolerant reader for the google/fonts `METADATA.pb` text-format protobuf.
//!
//! Only the fields the downloader needs are read: the family `name`, its
//! `license`, every `fonts { style weight filename }` block and every
//! `axes { tag min_value max_value }` block. Everything else (designer,
//! subsets, `source { files { ... } }`, ...) is skipped by brace depth. The
//! file comes from the network, so the parser never panics and treats garbage
//! as "fields absent" rather than an error: the planner then refuses a
//! document that lacks what it needs.

const MAX_DEPTH: usize = 16;
/// `METADATA.pb` files are ~1 KB; a megabyte is already hostile.
pub const MAX_METADATA_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetaFont {
    pub style: String,
    pub weight: u16,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetaAxis {
    pub tag: String,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub name: Option<String>,
    pub license: Option<String>,
    pub fonts: Vec<MetaFont>,
    pub axes: Vec<MetaAxis>,
}

impl Metadata {
    /// The `wght` axis range when the family ships variable files.
    pub fn weight_axis(&self) -> Option<(f32, f32)> {
        self.axes
            .iter()
            .find(|axis| axis.tag == "wght")
            .map(|axis| (axis.min, axis.max))
    }
}

/// Unquote a protobuf-text string value (`"abc"`); `None` when it is not one.
fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next()?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

pub fn parse_metadata(text: &str) -> Metadata {
    let mut meta = Metadata::default();
    let mut stack: Vec<&str> = Vec::new();
    let mut font = MetaFont::default();
    let mut axis: Option<(Option<String>, Option<f32>, Option<f32>)> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_suffix('{') {
            if stack.len() >= MAX_DEPTH {
                return Metadata::default();
            }
            let block = head.trim().trim_end_matches(':').trim();
            if stack.is_empty() {
                match block {
                    "fonts" => font = MetaFont::default(),
                    "axes" => axis = Some((None, None, None)),
                    _ => {}
                }
            }
            stack.push(block);
            continue;
        }
        if line == "}" {
            match stack.pop() {
                None => return Metadata::default(),
                Some(block) if stack.is_empty() => match block {
                    "fonts" => meta.fonts.push(std::mem::take(&mut font)),
                    "axes" => {
                        if let Some((Some(tag), Some(min), Some(max))) = axis.take() {
                            meta.axes.push(MetaAxis { tag, min, max });
                        }
                    }
                    _ => {}
                },
                Some(_) => {}
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match (stack.as_slice(), key) {
            ([], "name") => meta.name = unquote(value),
            ([], "license") => meta.license = unquote(value),
            (["fonts"], "style") => font.style = unquote(value).unwrap_or_default(),
            (["fonts"], "weight") => font.weight = value.parse::<u16>().unwrap_or(0),
            (["fonts"], "filename") => font.filename = unquote(value).unwrap_or_default(),
            (["axes"], "tag") => {
                if let Some(a) = axis.as_mut() {
                    a.0 = unquote(value);
                }
            }
            (["axes"], "min_value") => {
                if let Some(a) = axis.as_mut() {
                    a.1 = value.parse::<f32>().ok().filter(|v| v.is_finite());
                }
            }
            (["axes"], "max_value") => {
                if let Some(a) = axis.as_mut() {
                    a.2 = value.parse::<f32>().ok().filter(|v| v.is_finite());
                }
            }
            _ => {}
        }
    }
    meta
}
