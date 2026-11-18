//! Write-time text-contrast scan.
//!
//! For every text and icon node the scan needs the colour it sits on. That is
//! only knowable when the nearest painted ancestor is ONE solid colour. When it
//! is not (an image, a gradient, a translucent fill over something that is
//! itself unknown, a variable that does not resolve) the scan makes no
//! pass/fail guess: the text is reported as [`ContrastUncertain`] instead, so
//! the agent verifies it with a screenshot rather than trusting a number
//! computed against the wrong backdrop.
//!
//! Only ANCESTOR fills are tracked. An overlay that is a sibling painted over
//! an image is invisible to this scan.

use std::collections::BTreeMap;

use jian_ops_schema::node::TextContent;
use jian_ops_schema::variable::VariableDefinition;

use super::quality_scans::{ContrastIssue, CONTRAST_AA_TARGET, CONTRAST_ICON_TARGET};
use super::*;

type Variables<'a> = Option<&'a BTreeMap<String, VariableDefinition>>;
type Theme = BTreeMap<String, String>;

/// One sentence of advice that rides with every uncertain entry.
const UNCERTAIN_ADVICE: &str =
    "check with get_screenshot; add a scrim or solid backing if the text is hard to read";

/// Longest stretch of a text node's content echoed in an entry.
const UNCERTAIN_TEXT_CHARS: usize = 40;

/// Why the colour behind a text node cannot be reduced to one solid colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UncertainReason {
    Image,
    Gradient,
    Translucent,
    Unresolved,
}

impl UncertainReason {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Gradient => "gradient",
            Self::Translucent => "translucent",
            Self::Unresolved => "unresolved",
        }
    }
}

/// A text node whose backdrop the scan could not resolve; not a pass or a fail.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ContrastUncertain {
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub text: String,
    /// One of `image`, `gradient`, `translucent`, `unresolved`.
    pub reason: &'static str,
    pub advice: &'static str,
}

/// What the scan found: contrast failures against a known solid backdrop, and
/// text over a backdrop it could not resolve.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContrastScan {
    pub issues: Vec<ContrastIssue>,
    pub uncertain: Vec<ContrastUncertain>,
}

/// What the nearest painted ancestor contributes behind a node.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Backdrop {
    Solid(String),
    Uncertain(UncertainReason),
}

/// The contrast failures only (`scan_contrast`'s `issues`); the pre-existing
/// scan tests call this shape.
#[cfg(test)]
pub(super) fn scan_contrast_issues(
    nodes: &[PenNode],
    variables: Variables<'_>,
    theme: &Theme,
) -> Vec<ContrastIssue> {
    scan_contrast(nodes, variables, theme).issues
}

pub(super) fn scan_contrast(
    nodes: &[PenNode],
    variables: Variables<'_>,
    theme: &Theme,
) -> ContrastScan {
    let mut candidates = Vec::new();
    let mut uncertain = Vec::new();
    let mut backdrops = Vec::new();
    for node in nodes {
        collect_contrast_candidates(
            node,
            variables,
            theme,
            &mut backdrops,
            &mut candidates,
            &mut uncertain,
        );
    }

    let pairs: Vec<(String, String, f64)> = candidates
        .iter()
        .map(|candidate| (candidate.fg.clone(), candidate.bg.clone(), candidate.target))
        .collect();
    let report = op_ai_skills::color::contrast::scan_pairs(&pairs);
    let mut violations = report.violations.into_iter().peekable();
    let mut issues = Vec::new();
    for candidate in candidates {
        let Some(violation) = violations.peek() else {
            break;
        };
        if violation.fg == candidate.fg
            && violation.bg == candidate.bg
            && (violation.target - candidate.target).abs() < f64::EPSILON
        {
            let violation = violations.next().expect("peeked violation");
            issues.push(ContrastIssue {
                node_id: candidate.node_id,
                node_name: candidate.node_name,
                fg: violation.fg,
                bg: violation.bg,
                ratio: violation.ratio,
                target: violation.target,
            });
        }
    }
    ContrastScan { issues, uncertain }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ContrastCandidate {
    pub(super) node_id: String,
    pub(super) node_name: Option<String>,
    pub(super) fg: String,
    pub(super) bg: String,
    pub(super) target: f64,
}

pub(super) fn collect_contrast_candidates(
    node: &PenNode,
    variables: Variables<'_>,
    theme: &Theme,
    backdrops: &mut Vec<Backdrop>,
    out: &mut Vec<ContrastCandidate>,
    uncertain: &mut Vec<ContrastUncertain>,
) {
    let opacity = resolved_node_opacity(node, variables, theme);
    let pushed = container_backdrop(node, variables, theme, backdrops.last(), opacity);
    if let Some(backdrop) = pushed.as_ref() {
        backdrops.push(backdrop.clone());
    }

    match backdrops.last() {
        Some(Backdrop::Solid(bg)) => {
            let foreground = match node {
                PenNode::Text(text) => Some((&text.fill, CONTRAST_AA_TARGET)),
                PenNode::IconFont(icon) => Some((&icon.fill, CONTRAST_ICON_TARGET)),
                _ => None,
            };
            if let Some((fill, target)) = foreground {
                // A text node without a resolvable fill is not invisible: the
                // canvas paints it in a near-black default ink. That ink is
                // what has to contrast with the background, so check it
                // instead of skipping the node (the dark-surface case is where
                // such text really vanishes).
                let fg = first_solid_hex(fill, variables, theme, Some(bg), opacity).or_else(|| {
                    (matches!(node, PenNode::Text(_)) && fill_is_unresolved(fill))
                        .then(|| DEFAULT_TEXT_INK_HEX.to_string())
                });
                if let Some(fg) = fg {
                    out.push(ContrastCandidate {
                        node_id: node.id_str().to_string(),
                        node_name: node.base().name.clone(),
                        fg,
                        bg: bg.clone(),
                        target,
                    });
                }
            }
        }
        Some(Backdrop::Uncertain(reason)) => {
            // Text only: an icon over a photo is routine and a "check it"
            // line for each one would drown the entries that matter.
            if let PenNode::Text(text) = node {
                if let Some(snippet) = text_snippet(&text.content) {
                    uncertain.push(ContrastUncertain {
                        node_id: node.id_str().to_string(),
                        text: snippet,
                        reason: reason.as_str(),
                        advice: UNCERTAIN_ADVICE,
                    });
                }
            }
        }
        None => {}
    }

    if let Some(children) = node.children() {
        for child in children {
            collect_contrast_candidates(child, variables, theme, backdrops, out, uncertain);
        }
    }

    if pushed.is_some() {
        backdrops.pop();
    }
}

/// The first [`UNCERTAIN_TEXT_CHARS`] characters of a text node's content, or
/// `None` when it holds no visible text.
fn text_snippet(content: &TextContent) -> Option<String> {
    let full: String = match content {
        TextContent::Plain(text) => text.clone(),
        TextContent::Styled(segments) => segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect(),
    };
    let trimmed = full.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut snippet: String = trimmed.chars().take(UNCERTAIN_TEXT_CHARS).collect();
    if trimmed.chars().count() > UNCERTAIN_TEXT_CHARS {
        snippet.push_str("...");
    }
    Some(snippet)
}

/// The colour the canvas paints text with when no fill resolves
/// (`canvas_viewport_text.rs`: 0.08 grey, `#141414`).
const DEFAULT_TEXT_INK_HEX: &str = "#141414";

/// True when the fill list paints nothing the contrast scan can resolve to a
/// colour AND the renderer falls back to its default ink: no fill at all, or
/// only solid fills whose colour did not resolve. A gradient or image fill is
/// a deliberate paint and is left alone.
fn fill_is_unresolved(fill: &Option<Vec<PenFill>>) -> bool {
    fill.as_ref()
        .is_none_or(|fills| fills.iter().all(|paint| matches!(paint, PenFill::Solid(_))))
}

/// What `node` paints behind its children, given what is behind `node`.
///
/// `None`: the node paints no backdrop of its own (not a container, no fill, a
/// fully transparent fill) and children keep the ancestor's. Otherwise:
/// - an image fill, or a gradient / shader fill, is never one colour;
/// - a `$ref` colour that does not resolve is unknown;
/// - an opaque solid is that colour;
/// - a translucent solid (fill opacity, colour alpha or node opacity below 1)
///   blends over a solid parent exactly, over no parent against white, and over
///   an unknown parent stays unknown.
pub(super) fn container_backdrop(
    node: &PenNode,
    variables: Variables<'_>,
    theme: &Theme,
    parent: Option<&Backdrop>,
    node_opacity: f64,
) -> Option<Backdrop> {
    let fill = match node {
        PenNode::Frame(n) => &n.container.fill,
        PenNode::Group(n) => &n.container.fill,
        PenNode::Rectangle(n) => &n.container.fill,
        PenNode::Tabs(n) => &n.fill,
        _ => return None,
    };
    let paints = fill.as_ref()?;
    if paints
        .iter()
        .any(|paint| matches!(paint, PenFill::Image(_)))
    {
        return Some(Backdrop::Uncertain(UncertainReason::Image));
    }
    if paints.iter().any(|paint| {
        matches!(
            paint,
            PenFill::LinearGradient(_)
                | PenFill::RadialGradient(_)
                | PenFill::MeshGradient(_)
                | PenFill::Shader(_)
        )
    }) {
        return Some(Backdrop::Uncertain(UncertainReason::Gradient));
    }
    let body = paints.iter().find_map(|paint| match paint {
        PenFill::Solid(body) => Some(body),
        _ => None,
    })?;
    let Some(resolved) =
        op_editor_core::variables_resolve::resolve_color_ref(&body.color, variables, theme)
    else {
        return Some(Backdrop::Uncertain(UncertainReason::Unresolved));
    };
    let resolved = resolved.trim();
    // `transparent`, `none` and other non-hex values paint nothing.
    op_editor_core::parse_hex_rgb(resolved)?;
    let layer_opacity = node_opacity * f64::from(body.opacity.unwrap_or(1.0));
    let alpha = layer_opacity * f64::from(op_editor_core::parse_hex_alpha(resolved));
    if alpha <= 0.0 {
        return None;
    }
    if alpha >= 0.999 {
        return composite_color(resolved, "#FFFFFF", 1.0).map(Backdrop::Solid);
    }
    match parent {
        Some(Backdrop::Uncertain(_)) => Some(Backdrop::Uncertain(UncertainReason::Translucent)),
        Some(Backdrop::Solid(below)) => {
            composite_color(resolved, below, layer_opacity).map(Backdrop::Solid)
        }
        None => composite_color(resolved, "#FFFFFF", layer_opacity).map(Backdrop::Solid),
    }
}

pub(super) fn first_solid_hex(
    fill: &Option<Vec<PenFill>>,
    variables: Variables<'_>,
    theme: &Theme,
    background: Option<&str>,
    node_opacity: f64,
) -> Option<String> {
    fill.as_ref()?.iter().find_map(|fill| match fill {
        PenFill::Solid(body) => resolved_hex(
            &body.color,
            variables,
            theme,
            background,
            node_opacity * f64::from(body.opacity.unwrap_or(1.0)),
        ),
        _ => None,
    })
}

pub(super) fn resolved_node_opacity(
    node: &PenNode,
    variables: Variables<'_>,
    theme: &Theme,
) -> f64 {
    match node.base().opacity.as_ref() {
        Some(NumberOrExpression::Number(opacity)) => opacity.clamp(0.0, 1.0),
        Some(NumberOrExpression::Expression(reference)) => {
            op_editor_core::variables_resolve::resolve_numeric_ref(reference, variables, theme)
                .unwrap_or(1.0)
                .clamp(0.0, 1.0)
        }
        None => 1.0,
    }
}

pub(super) fn resolved_hex(
    color: &str,
    variables: Variables<'_>,
    theme: &Theme,
    background: Option<&str>,
    opacity: f64,
) -> Option<String> {
    let resolved = op_editor_core::variables_resolve::resolve_color_ref(color, variables, theme)?;
    composite_color(resolved.trim(), background.unwrap_or("#FFFFFF"), opacity)
}

pub(super) fn composite_color(foreground: &str, background: &str, opacity: f64) -> Option<String> {
    let (fg_r, fg_g, fg_b) = op_editor_core::parse_hex_rgb(foreground)?;
    let (bg_r, bg_g, bg_b) = op_editor_core::parse_hex_rgb(background)?;
    let alpha = f64::from(op_editor_core::parse_hex_alpha(foreground)) * opacity.clamp(0.0, 1.0);
    let blend = |foreground: f32, background: f32| {
        ((f64::from(foreground) * alpha + f64::from(background) * (1.0 - alpha)) * 255.0).round()
            as u8
    };
    Some(format!(
        "#{:02X}{:02X}{:02X}",
        blend(fg_r, bg_r),
        blend(fg_g, bg_g),
        blend(fg_b, bg_b)
    ))
}
