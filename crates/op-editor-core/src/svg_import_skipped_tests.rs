//! `skipped_svg_features` reports what `import_svg` drops. Each test pairs the
//! report with the import itself, so the report cannot drift from the importer:
//! what is claimed dropped really does not reach the document.

#![cfg(test)]

use crate::pen_node_ext::PenNodeExt;
use crate::svg_import::{skipped_svg_features, SkippedSvgFeature};
use crate::test_support::state_with;
use jian_ops_schema::node::PenNode;
use jian_ops_schema::style::PenFill;

fn skipped(svg: &str) -> Vec<(String, usize)> {
    skipped_svg_features(svg)
        .into_iter()
        .map(|SkippedSvgFeature { feature, count }| (feature, count))
        .collect()
}

fn count_of(report: &[(String, usize)], feature: &str) -> usize {
    report
        .iter()
        .find(|(name, _)| name == feature)
        .map_or(0, |(_, count)| *count)
}

/// Every node the import put on the page, depth first.
fn imported_ids(svg: &str) -> Vec<String> {
    fn walk(nodes: &[PenNode], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.id_str().to_string());
            if let Some(children) = node.children() {
                walk(children, out);
            }
        }
    }
    let mut state = state_with(vec![]);
    let mut next = 1u64;
    state.import_svg(&mut next, svg, (0.0, 0.0));
    let mut ids = Vec::new();
    walk(state.active_children(), &mut ids);
    ids
}

#[test]
fn a_plain_icon_with_shapes_arcs_and_groups_reports_nothing() {
    let svg = r##"<svg viewBox="0 0 24 24" fill="none" stroke="#111111">
        <g><path d="M3 12a9 9 0 1 0 18 0a9 9 0 1 0-18 0"/><circle cx="12" cy="12" r="3"/></g>
    </svg>"##;
    assert!(skipped(svg).is_empty(), "{:?}", skipped(svg));
    assert!(skipped("not an svg at all").is_empty());
}

#[test]
fn transform_is_reported_and_really_ignored() {
    let svg = r#"<svg viewBox="0 0 100 100">
        <g transform="translate(50 50)"><rect width="10" height="10" transform="rotate(45)"/></g>
    </svg>"#;
    assert_eq!(count_of(&skipped(svg), "transform"), 2);
    // The same drawing without a transform imports to the same node count.
    let plain = svg.replace(" transform=\"translate(50 50)\"", "");
    let plain = plain.replace(" transform=\"rotate(45)\"", "");
    assert_eq!(imported_ids(svg).len(), imported_ids(&plain).len());
}

#[test]
fn text_use_image_and_unknown_elements_are_reported_and_produce_no_node() {
    let with_extras = r##"<svg viewBox="0 0 100 100">
        <defs><circle id="dot" r="5"/></defs>
        <text x="1" y="2">Hello</text>
        <use href="#dot"/>
        <image href="a.png" width="5" height="5"/>
        <a href="#"><rect width="9" height="9"/></a>
        <rect width="10" height="10"/>
    </svg>"##;
    let report = skipped(with_extras);
    assert_eq!(count_of(&report, "text"), 1, "{report:?}");
    assert_eq!(count_of(&report, "use"), 1, "{report:?}");
    assert_eq!(count_of(&report, "image"), 1, "{report:?}");
    assert_eq!(count_of(&report, "element:a"), 1, "{report:?}");
    // Only the one real rect lands: the rect inside `<a>` is dropped with it.
    let only_rect = r#"<svg viewBox="0 0 100 100"><rect width="10" height="10"/></svg>"#;
    assert_eq!(
        imported_ids(with_extras).len(),
        imported_ids(only_rect).len()
    );
}

#[test]
fn gradients_are_reported_by_definition_and_by_paint_and_the_fill_turns_black() {
    let svg = r##"<svg viewBox="0 0 100 100">
        <defs><linearGradient id="a"><stop offset="0"/></linearGradient>
              <radialGradient id="b"/></defs>
        <rect width="50" height="50" fill="url(#a)"/>
        <rect width="50" height="50" stroke="url(#b)"/>
    </svg>"##;
    let report = skipped(svg);
    assert_eq!(count_of(&report, "gradient"), 2, "{report:?}");
    assert_eq!(count_of(&report, "gradient-paint"), 2, "{report:?}");
    // What the report is warning about: the gradient fill is drawn black.
    let mut state = state_with(vec![]);
    let mut next = 1u64;
    state.import_svg(&mut next, svg, (0.0, 0.0));
    let first_fill = match state.active_children()[0].children().map(|kids| &kids[0]) {
        Some(PenNode::Rectangle(rect)) => rect.container.fill.clone(),
        other => panic!("expected the first rect, got {other:?}"),
    };
    match first_fill.as_deref() {
        Some([PenFill::Solid(solid)]) => assert!(solid.color.eq_ignore_ascii_case("#000000")),
        other => panic!("expected a black solid fill, got {other:?}"),
    }
}

#[test]
fn css_opacity_stroke_style_clip_and_corner_radius_are_reported() {
    let svg = r##"<svg viewBox="0 0 100 100" stroke-linecap="round">
        <style>.a{fill:red}</style>
        <clipPath id="c"><rect width="5" height="5"/></clipPath>
        <rect class="a" width="10" height="10" rx="3" opacity="0.5" clip-path="url(#c)"/>
        <rect width="10" height="10" rx="0"/>
    </svg>"##;
    let report = skipped(svg);
    assert_eq!(count_of(&report, "css-style"), 1, "{report:?}");
    assert_eq!(count_of(&report, "clip-mask-filter"), 2, "{report:?}");
    assert_eq!(count_of(&report, "opacity"), 1, "{report:?}");
    assert_eq!(count_of(&report, "stroke-style"), 1, "{report:?}");
    // Only the rect with a positive radius counts.
    assert_eq!(count_of(&report, "rect-corner-radius"), 1, "{report:?}");
}

#[test]
fn the_report_is_ordered_by_count_and_capped() {
    let mut svg = String::from(r#"<svg viewBox="0 0 100 100">"#);
    for tag in ["a", "b", "c", "d", "e", "f", "g2", "h", "i", "j"] {
        svg.push_str(&format!("<{tag}/>"));
    }
    svg.push_str("<text/><text/><text/></svg>");
    let report = skipped(&svg);
    assert_eq!(report.len(), 8, "{report:?}");
    assert_eq!(report[0], ("text".to_string(), 3));
    // Ties sort by name.
    assert_eq!(report[1].0, "element:a");
}

#[test]
fn arcs_in_a_24_unit_viewbox_keep_their_flags_when_the_drawing_is_scaled() {
    // The research claim "arcs import correctly" holds: the radii and the end
    // point scale with the 24 -> 240 px root scale (a bare 24-unit viewBox imports at 24 px), the rotation and the two
    // flags do not.
    let svg = r#"<svg viewBox="0 0 24 24" width="240" height="240"><path d="M3 12a9 9 0 1 0 18 0"/></svg>"#;
    let mut state = state_with(vec![]);
    let mut next = 1u64;
    state.import_svg(&mut next, svg, (0.0, 0.0));
    // The import wraps what it builds in one Group.
    let imported = state.active_children()[0].children().expect("import group");
    let PenNode::Path(path) = &imported[0] else {
        panic!("expected one path");
    };
    let d = path.d.as_deref().expect("d");
    let arc: Vec<&str> = d
        .split(|c: char| c.is_whitespace() || c == ',')
        .skip_while(|token| !token.eq_ignore_ascii_case("a"))
        .skip(1)
        .take(7)
        .collect();
    assert_eq!(arc.len(), 7, "{d}");
    let radius: f64 = arc[0].parse().expect("rx");
    assert!((radius - 9.0 * 10.0).abs() < 0.01, "{d}");
    assert_eq!(&arc[2..5], ["0", "1", "0"], "{d}");
}
