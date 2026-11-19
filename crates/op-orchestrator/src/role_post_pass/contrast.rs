//! I3 contrast cluster — `fixButtonForegroundContrast`,
//! the container/descendant text-contrast fixes and `fixSectionAlternation`.

use op_design_lint::color::color_contrast;

use super::*;

pub(super) fn fix_button_foreground_contrast(node: &mut Value) {
    if !matches!(role_of(node), Some("button") | Some("icon-button")) {
        return;
    }
    // A transparent button has no bg to compute contrast against.
    if !has_visible_fill(node) {
        return;
    }
    // An image, gradient or translucent button background is not one colour:
    // leave its content alone rather than recolour it against a guess.
    let Some(bg_raw) = opaque_solid_background(node) else {
        return;
    };
    // A brand-accent token (`$--primary` / `$--destructive`) binds to a
    // concrete hex only at render time, so `resolve_color_maybe_ref` can't
    // read its luminance here and the pass used to bail — leaving the model's
    // default-dark icon on an orange accent button (measured: a `sliders`
    // icon at `#0F172A` on a `$--primary` filter button). These tokens are
    // always saturated colours that need a WHITE foreground, so treat the bg
    // as dark and let the same override logic below flip the children.
    let bg = match resolve_color_maybe_ref(&bg_raw) {
        Some(bg) if hex_luminance(&bg).is_some() => bg,
        Some(_) => return, // unparseable bg → can't pick safely, skip
        None if is_saturated_accent_token(&bg_raw) => "#EA580C".to_string(),
        None => return,
    };

    let fg = preferred_foreground_for_bg(&bg);
    let fg_fill = solid_fill(fg);

    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    // PASS 1: a sibling text's resolved color is the reference foreground —
    // text + icon in a button read as one unit.
    let mut reference_fg: Option<String> = None;
    for child in children {
        if child.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if !has_visible_fill(child) {
            continue;
        }
        if let Some(tc) = get_first_solid_color(child) {
            if let Some(resolved) = resolve_color_maybe_ref(&tc) {
                reference_fg = Some(resolved);
                break;
            }
        }
    }
    let final_fg_fill = reference_fg
        .as_ref()
        .map(|c| solid_fill(c))
        .unwrap_or_else(|| fg_fill.clone());

    // PASS 2: apply foreground to children.
    let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) else {
        return;
    };
    for child in children.iter_mut() {
        match child.get("type").and_then(Value::as_str) {
            Some("text") => {
                if !has_visible_fill(child) {
                    child["fill"] = fg_fill.clone();
                }
            }
            Some("icon_font") => {
                if !has_visible_fill(child) {
                    child["fill"] = final_fg_fill.clone();
                    continue;
                }
                if let Some(reference) = &reference_fg {
                    let existing =
                        get_first_solid_color(child).and_then(|c| resolve_color_maybe_ref(&c));
                    if let Some(existing) = existing {
                        if existing.to_lowercase() != reference.to_lowercase() {
                            child["fill"] = final_fg_fill.clone();
                        }
                    }
                } else {
                    // Icon-only button: luminance-delta override.
                    let existing =
                        get_first_solid_color(child).and_then(|c| resolve_color_maybe_ref(&c));
                    if let Some(existing) = existing {
                        if needs_luminance_contrast_override(&existing, &bg) {
                            child["fill"] = fg_fill.clone();
                        }
                    }
                }
            }
            Some("path") => {
                let has_stroke = child.get("stroke").map(|s| !s.is_null()).unwrap_or(false);
                let has_stroke_fill = child
                    .get("stroke")
                    .and_then(|s| s.get("fill"))
                    .and_then(Value::as_array)
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if has_visible_fill(child) {
                    // already styled
                } else if has_stroke && !has_stroke_fill {
                    child["stroke"]["fill"] = final_fg_fill.clone();
                } else if !has_stroke {
                    child["fill"] = final_fg_fill.clone();
                }
            }
            _ => {}
        }
    }
}

// ── fixContainerTextContrast ────────────────────────────────────────────────

pub(super) fn fix_container_text_contrast(node: &mut Value) {
    if matches!(role_of(node), Some("button") | Some("icon-button")) {
        return;
    }
    fix_container_text_contrast_for_current(node);
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            fix_container_text_contrast(child);
        }
    }
}

pub(super) fn fix_container_text_contrast_for_current(node: &mut Value) {
    if node.get("type").and_then(Value::as_str) != Some("frame") {
        return;
    }
    if !has_visible_fill(node) {
        return;
    }
    // Only an opaque solid backdrop is judged: text over an image, a gradient
    // or a translucent overlay is left as authored (the write-time scan
    // reports it as uncertain instead).
    let Some(bg_raw) = opaque_solid_background(node) else {
        return;
    };
    let Some(bg) = resolve_color_maybe_ref(&bg_raw) else {
        return;
    };
    if hex_luminance(&bg).is_none() {
        return;
    }

    // The text colours this container already uses are the palette a repair
    // draws from first; they are gathered before any text is recoloured.
    let mut palette: Vec<String> = Vec::new();
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_descendant_text_colors(child, &mut palette);
        }
    }
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            fix_descendant_text_contrast(child, &bg, &palette);
        }
    }
}

/// WCAG contrast target for one text node: 3:1 for large text (>= 24 px, or
/// >= 19 px at weight >= 700, the lint's own definition), 4.5:1 otherwise.
fn text_contrast_target(text: &Value) -> f64 {
    let size = text.get("fontSize").and_then(Value::as_f64).unwrap_or(0.0);
    let weight = match text.get("fontWeight") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    if size >= 24.0 || (size >= 19.0 && weight.is_some_and(|w| w >= 700.0)) {
        3.0
    } else {
        4.5
    }
}

/// Resolved hex colours of the text nodes this pass judges against one
/// backdrop (same walk as [`fix_descendant_text_contrast`]).
fn collect_descendant_text_colors(node: &Value, out: &mut Vec<String>) {
    if node.get("type").and_then(Value::as_str) == Some("text") {
        if let Some(color) = get_first_solid_color(node).and_then(|c| resolve_color_maybe_ref(&c)) {
            if hex_luminance(&color).is_some() && !out.contains(&color) {
                out.push(color);
            }
        }
        return;
    }
    if matches!(role_of(node), Some("button") | Some("icon-button"))
        || paints_unjudgeable_backdrop(node)
    {
        return;
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            collect_descendant_text_colors(child, out);
        }
    }
}

/// The replacement foreground for text that fails `target` on `bg`, in
/// preference order: a text colour the container already uses, then the
/// pass's own dark / white, then pure black / white (the only pair that always
/// clears 4.5:1). Within a tier the best ratio wins; a tier is used only when
/// its best candidate meets the target, and the last tier is used regardless.
fn readable_foreground(bg: &str, target: f64, palette: &[String]) -> String {
    let palette_tier: Vec<&str> = palette.iter().map(String::as_str).collect();
    for tier in [palette_tier.as_slice(), &["#0F172A", "#FFFFFF"]] {
        if let Some((color, ratio)) = best_contrast(tier, bg) {
            if ratio >= target {
                return color.to_string();
            }
        }
    }
    best_contrast(&["#000000", "#FFFFFF"], bg)
        .map(|(color, _)| color.to_string())
        .unwrap_or_else(|| "#000000".to_string())
}

/// The candidate with the highest WCAG ratio against `bg`.
fn best_contrast<'a>(candidates: &[&'a str], bg: &str) -> Option<(&'a str, f64)> {
    candidates
        .iter()
        .map(|candidate| (*candidate, color_contrast(candidate, bg)))
        .filter(|(_, ratio)| ratio.is_finite())
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

pub(super) fn fix_descendant_text_contrast(node: &mut Value, bg: &str, palette: &[String]) {
    if node.get("type").and_then(Value::as_str) == Some("text") {
        let Some(existing) =
            get_first_solid_color(node).and_then(|color| resolve_color_maybe_ref(&color))
        else {
            return;
        };
        let ratio = color_contrast(&existing, bg);
        if !ratio.is_finite() {
            return;
        }
        let target = text_contrast_target(node);
        // The authored colour already reads: keep it. Only text that fails
        // the WCAG ratio is recoloured, so a dark label on a mid-tone bar
        // is not flipped to white by a luminance rule.
        if ratio >= target {
            return;
        }
        node["fill"] = solid_fill(&readable_foreground(bg, target, palette));
        return;
    }
    if matches!(role_of(node), Some("button") | Some("icon-button")) {
        return;
    }
    // A nested container with its own image / gradient / translucent fill
    // changes what the text behind it sits on; the outer colour no longer
    // applies, so its whole subtree is left alone.
    if paints_unjudgeable_backdrop(node) {
        return;
    }
    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            fix_descendant_text_contrast(child, bg, palette);
        }
    }
}

// ── fixSectionAlternation ────────────────────────────────────────────────────

pub(super) const SECTION_ROLES: &[&str] =
    &["section", "hero", "cta-section", "stats-section", "footer"];
pub(super) const ALTERNATING_BG: [&str; 2] = ["#FFFFFF", "#F8FAFC"];

pub(super) fn fix_section_alternation(node: &mut Value) {
    if node.get("layout").and_then(Value::as_str) != Some("vertical") {
        return;
    }
    // Only alternate on light-themed pages (the hardcoded white strips would
    // fight a dark page background).
    if let Some(bg) = get_first_solid_color(node) {
        if hex_luminance(&bg).map(|l| l < 0.5).unwrap_or(false) {
            return;
        }
    }
    let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) else {
        return;
    };
    // Group consecutive section-role children into runs.
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for (i, child) in children.iter().enumerate() {
        let is_section = child.get("type").and_then(Value::as_str) == Some("frame")
            && role_of(child)
                .map(|r| SECTION_ROLES.contains(&r))
                .unwrap_or(false);
        if is_section {
            current.push(i);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    for run in runs {
        let unfilled = run.iter().filter(|&&i| !has_fill(&children[i])).count();
        if unfilled < 3 {
            continue;
        }
        let mut idx = 0;
        for &i in &run {
            if !has_fill(&children[i]) {
                children[i]["fill"] = solid_fill(ALTERNATING_BG[idx % 2]);
                idx += 1;
            }
        }
    }
}

// ── fixOrphanContainerContrast ───────────────────────────────────────────────
