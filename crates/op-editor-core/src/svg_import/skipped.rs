//! What the importer leaves out. [`skipped_svg_features`] walks the document
//! with the importer's own scanner and counts the features it does not carry
//! over, so a caller can say so instead of handing back a plainer drawing in
//! silence. It reads the SVG only: what gets imported is not changed.

use std::collections::BTreeMap;

use super::*;

/// One feature the import dropped and how many times the document used it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSvgFeature {
    pub feature: String,
    pub count: usize,
}

/// Most distinct features one report names.
const MAX_REPORTED_FEATURES: usize = 8;

/// Shapes the importer converts to nodes.
const IMPORTED_SHAPES: &[&str] = &[
    "rect", "circle", "ellipse", "line", "polyline", "polygon", "path",
];

/// Presentation attributes no import step reads.
const OPACITY_ATTRS: &[&str] = &["opacity", "fill-opacity", "stroke-opacity"];
const STROKE_STYLE_ATTRS: &[&str] = &[
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-miterlimit",
];
const EFFECT_ATTRS: &[&str] = &["clip-path", "mask", "filter"];

/// The features of `svg` the importer drops, most used first (ties by name),
/// at most [`MAX_REPORTED_FEATURES`]. Empty when nothing is lost or the text
/// holds no `<svg>` element.
pub fn skipped_svg_features(svg: &str) -> Vec<SkippedSvgFeature> {
    let Some((body, root_attrs)) = extract_svg_root(svg) else {
        return Vec::new();
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    note_attrs(&root_attrs, &mut counts);
    let tree = parse_svg_tree_observing(body, &mut |tag, inner| {
        note_skipped_element(tag, inner, &mut counts);
    });
    for element in &tree {
        note_tree_element(element, &mut counts);
    }
    let mut features: Vec<SkippedSvgFeature> = counts
        .into_iter()
        .map(|(feature, count)| SkippedSvgFeature { feature, count })
        .collect();
    features.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.feature.cmp(&b.feature))
    });
    features.truncate(MAX_REPORTED_FEATURES);
    features
}

fn bump(counts: &mut BTreeMap<String, usize>, feature: &str) {
    *counts.entry(feature.to_string()).or_insert(0) += 1;
}

/// The feature a [`SKIP_TAGS`] element stands for. Plain metadata
/// (`title`, `desc`, `metadata`, `script`) and the containers that only hold
/// other skipped elements are not features.
fn feature_for_skipped_tag(tag: &str) -> Option<&'static str> {
    match tag {
        "lineargradient" | "radialgradient" => Some("gradient"),
        "style" => Some("css-style"),
        "clippath" | "mask" | "filter" => Some("clip-mask-filter"),
        "animate" | "animatemotion" | "set" => Some("animation"),
        "foreignobject" => Some("foreignObject"),
        _ => None,
    }
}

/// Count a skipped element, and the skipped elements nested in a skipped
/// container (a gradient lives inside `<defs>`, which the importer drops whole).
fn note_skipped_element(tag: &str, inner: &str, counts: &mut BTreeMap<String, usize>) {
    if let Some(feature) = feature_for_skipped_tag(tag) {
        bump(counts, feature);
    }
    if !matches!(tag, "defs" | "symbol" | "pattern" | "marker") {
        return;
    }
    for nested in tag_names(inner) {
        if let Some(feature) = feature_for_skipped_tag(&nested) {
            bump(counts, feature);
        }
    }
}

/// Lower-case names of the opening tags in `markup`, in order.
fn tag_names(markup: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = markup;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        if rest.starts_with(['/', '!', '?']) {
            continue;
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        names.push(rest[..end].to_ascii_lowercase());
    }
    names
}

fn note_tree_element(element: &SvgTree, counts: &mut BTreeMap<String, usize>) {
    match element.tag.as_str() {
        "g" | "svg" => {
            note_attrs(&element.attrs, counts);
            for child in &element.children {
                note_tree_element(child, counts);
            }
        }
        tag if IMPORTED_SHAPES.contains(&tag) => {
            note_attrs(&element.attrs, counts);
            let rounded = ["rx", "ry"].iter().any(|key| {
                element
                    .attr(key)
                    .and_then(|value| value.trim().parse::<f64>().ok())
                    .is_some_and(|radius| radius > 0.0)
            });
            if tag == "rect" && rounded {
                bump(counts, "rect-corner-radius");
            }
        }
        // The importer converts none of these, and does not look inside them.
        "text" | "tspan" => bump(counts, "text"),
        "use" => bump(counts, "use"),
        "image" => bump(counts, "image"),
        other => bump(counts, &format!("element:{other}")),
    }
}

/// Attributes on an element that is imported (or that wraps imported
/// elements): the ones no import step reads.
fn note_attrs(attrs: &[(String, String)], counts: &mut BTreeMap<String, usize>) {
    let has = |name: &str| extract_style_or_attr(attrs, name).is_some();
    if attrs.iter().any(|(key, _)| key == "transform") {
        bump(counts, "transform");
    }
    if OPACITY_ATTRS.iter().any(|name| has(name)) {
        bump(counts, "opacity");
    }
    if STROKE_STYLE_ATTRS.iter().any(|name| has(name)) {
        bump(counts, "stroke-style");
    }
    if EFFECT_ATTRS.iter().any(|name| has(name)) {
        bump(counts, "clip-mask-filter");
    }
    let paints_with_url = ["fill", "stroke"].iter().any(|name| {
        extract_style_or_attr(attrs, name)
            .is_some_and(|value| value.trim().to_ascii_lowercase().starts_with("url("))
    });
    if paints_with_url {
        bump(counts, "gradient-paint");
    }
}
