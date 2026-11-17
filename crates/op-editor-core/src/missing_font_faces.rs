//! Which faces (weight + italic) of one family a document actually uses.
//!
//! `FontPlan` (jian-ops-schema) records codepoints per family but not weights,
//! so the downloader asks this scan instead: it fetches exactly the faces the
//! document needs rather than a whole family.

use std::collections::BTreeSet;

use jian_ops_schema::node::text::{FontStyleKind, FontWeight, TextContent};
use jian_ops_schema::node::PenNode;

use crate::font_catalog::primary_concrete_font_family;
use crate::pen_node_ext::PenNodeExt;
use crate::EditorState;

/// `(weight snapped to 100..=900 in steps of 100, italic)`.
pub type FontFace = (u16, bool);

const REGULAR: FontFace = (400, false);

fn snap(weight: u32) -> u16 {
    (((weight.clamp(100, 900) + 50) / 100) * 100).min(900) as u16
}

fn keyword_weight(word: &str) -> Option<u32> {
    let word = word.trim().to_ascii_lowercase();
    if let Ok(number) = word.parse::<u32>() {
        return Some(number);
    }
    Some(match word.as_str() {
        "hairline" | "thin" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "normal" | "regular" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "black" | "heavy" => 900,
        _ => return None,
    })
}

fn is_family(stack: &str, family: &str) -> bool {
    primary_concrete_font_family(stack).is_some_and(|primary| primary.eq_ignore_ascii_case(family))
}

fn scan_nodes(nodes: &[PenNode], family: &str, faces: &mut BTreeSet<FontFace>) {
    for node in nodes {
        match node {
            PenNode::Text(text) => {
                let node_family = text.font_family.as_deref();
                let node_weight = match &text.font_weight {
                    Some(FontWeight::Number(n)) => Some(*n),
                    Some(FontWeight::Keyword(word)) => keyword_weight(word),
                    None => None,
                };
                let node_italic = matches!(text.font_style, Some(FontStyleKind::Italic));
                let mut push = |stack: Option<&str>, weight: Option<u32>, italic: bool| {
                    if stack.is_some_and(|stack| is_family(stack, family)) {
                        faces.insert((weight.map_or(400, snap_u32), italic));
                    }
                };
                match &text.content {
                    TextContent::Plain(_) => push(node_family, node_weight, node_italic),
                    TextContent::Styled(segments) => {
                        for segment in segments {
                            push(
                                segment.font_family.as_deref().or(node_family),
                                segment.font_weight.or(node_weight),
                                segment.font_style.as_ref().map_or(node_italic, |s| {
                                    matches!(s, jian_ops_schema::style::FontStyleKind::Italic)
                                }),
                            );
                        }
                    }
                }
            }
            PenNode::Ref(reference) => {
                if let Some(descendants) = &reference.descendants {
                    for value in descendants.values() {
                        scan_json(value, family, faces);
                    }
                }
            }
            _ => {}
        }
        if let Some(children) = node.children() {
            scan_nodes(children, family, faces);
        }
    }
}

fn snap_u32(weight: u32) -> u16 {
    snap(weight)
}

fn scan_json(value: &serde_json::Value, family: &str, faces: &mut BTreeSet<FontFace>) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(stack) = object.get("fontFamily").and_then(|v| v.as_str()) {
                if is_family(stack, family) {
                    let weight = match object.get("fontWeight") {
                        Some(serde_json::Value::Number(n)) => n.as_u64().map(|n| n as u32),
                        Some(serde_json::Value::String(word)) => keyword_weight(word),
                        _ => None,
                    };
                    let italic = object.get("fontStyle").and_then(|v| v.as_str()) == Some("italic");
                    faces.insert((weight.map_or(400, snap_u32), italic));
                }
            }
            for child in object.values() {
                scan_json(child, family, faces);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                scan_json(item, family, faces);
            }
        }
        _ => {}
    }
}

/// The faces of `family` (compared like detection does: primary concrete
/// family of each stack, ASCII case-insensitive) used anywhere in the
/// document, including styled segments and component-instance overrides.
/// Never empty: a family whose usage cannot be read counts as regular.
pub fn document_font_faces(state: &EditorState, family: &str) -> BTreeSet<FontFace> {
    let mut faces = BTreeSet::new();
    scan_nodes(&state.doc.children, family, &mut faces);
    if let Some(pages) = &state.doc.pages {
        for page in pages {
            scan_nodes(&page.children, family, &mut faces);
        }
    }
    if faces.is_empty() {
        faces.insert(REGULAR);
    }
    faces
}

#[cfg(test)]
#[path = "missing_font_faces_tests.rs"]
mod tests;
