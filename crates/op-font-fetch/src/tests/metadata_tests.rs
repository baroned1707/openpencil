use std::collections::BTreeSet;

use crate::fake::{metadata_static, metadata_variable};
use crate::metadata::parse_metadata;
use crate::plan::{plan_files, FaceRequest};

const INTER_PB: &str = r#"name: "Inter"
designer: "Rasmus Andersson"
license: "OFL"
category: "SANS_SERIF"
fonts {
  name: "Inter"
  style: "normal"
  weight: 400
  filename: "Inter[opsz,wght].ttf"
  post_script_name: "Inter-Regular"
}
fonts {
  name: "Inter"
  style: "italic"
  weight: 400
  filename: "Inter-Italic[opsz,wght].ttf"
}
subsets: "latin"
axes {
  tag: "opsz"
  min_value: 14.0
  max_value: 32.0
}
axes {
  tag: "wght"
  min_value: 100.0
  max_value: 900.0
}
source {
  repository_url: "https://www.github.com/rsms/inter"
  files {
    source_file: "Inter[opsz,wght].ttf"
    dest_file: "Inter[opsz,wght].ttf"
  }
}
"#;

fn faces(list: &[(u16, bool)]) -> BTreeSet<FaceRequest> {
    list.iter()
        .map(|&(weight, italic)| FaceRequest { weight, italic })
        .collect()
}

#[test]
fn metadata_pb_parser_reads_the_fields_the_downloader_needs() {
    let meta = parse_metadata(INTER_PB);
    assert_eq!(meta.name.as_deref(), Some("Inter"));
    assert_eq!(meta.license.as_deref(), Some("OFL"));
    assert_eq!(meta.fonts.len(), 2);
    assert_eq!(meta.fonts[1].style, "italic");
    assert_eq!(meta.fonts[0].filename, "Inter[opsz,wght].ttf");
    assert_eq!(meta.weight_axis(), Some((100.0, 900.0)));
}

#[test]
fn metadata_pb_parser_tolerates_garbage() {
    let deep = "a {\n".repeat(40);
    let cases: Vec<String> = vec![
        String::new(),
        "}\n}\n}".into(),
        "{{{{".into(),
        deep,
        "name: \"unterminated".into(),
        "fonts {\n weight: notanumber\n filename: 5\n}".into(),
        "axes {\n tag: \"wght\"\n min_value: NaN\n max_value: inf\n}".into(),
        "\u{0}\u{1}\u{2}: : :\n{\n}".into(),
        "name: \"a\\".into(),
        "license: 'OFL'\nfonts {".into(),
    ];
    for case in cases {
        // Must not panic; whatever it returns, the planner must cope.
        let meta = parse_metadata(&case);
        let _ = plan_files(&meta, &BTreeSet::new());
    }
    assert!(parse_metadata("name: \"unterminated").name.is_none());
    assert_eq!(
        parse_metadata("axes {\n tag: \"wght\"\n min_value: NaN\n max_value: 900\n}").weight_axis(),
        None
    );
}

#[test]
fn variable_family_fetches_one_file_per_requested_style() {
    let meta = parse_metadata(INTER_PB);
    let one = plan_files(&meta, &faces(&[(400, false), (700, false), (300, false)])).unwrap();
    assert_eq!(one.len(), 1, "a variable file serves every weight: {one:?}");
    assert_eq!(one[0].file_name, "Inter[opsz,wght].ttf");
    let both = plan_files(&meta, &faces(&[(400, false), (700, true)])).unwrap();
    assert_eq!(both.len(), 2);
    assert!(both.iter().any(|f| f.italic));
}

#[test]
fn static_family_fetches_each_requested_weight_and_no_others() {
    let text = metadata_static(
        "Lobster Two",
        "OFL",
        &[
            ("L-Regular.ttf", "normal", 400),
            ("L-Bold.ttf", "normal", 700),
            ("L-Italic.ttf", "italic", 400),
            ("L-BoldItalic.ttf", "italic", 700),
        ],
    );
    let meta = parse_metadata(&text);
    let names = |wanted| -> Vec<String> {
        plan_files(&meta, &wanted)
            .unwrap()
            .into_iter()
            .map(|f| f.file_name)
            .collect()
    };
    assert_eq!(names(faces(&[(700, false)])), ["L-Bold.ttf"]);
    assert_eq!(
        names(faces(&[(400, false), (700, false)])),
        ["L-Regular.ttf", "L-Bold.ttf"]
    );
    // 600 snaps to the nearest offered weight, ties to the lighter one.
    assert_eq!(names(faces(&[(600, false)])), ["L-Bold.ttf"]);
    assert_eq!(names(faces(&[(500, false)])), ["L-Regular.ttf"]);
    assert_eq!(names(faces(&[(700, true)])), ["L-BoldItalic.ttf"]);
    // Nothing requested = the regular face.
    assert_eq!(names(BTreeSet::new()), ["L-Regular.ttf"]);
}

#[test]
fn missing_style_falls_back_to_the_regular_face_so_the_row_can_resolve() {
    let meta = parse_metadata(&metadata_variable(
        "Roboto Slab",
        "APACHE2",
        &[("RobotoSlab[wght].ttf", "normal")],
    ));
    let plan = plan_files(&meta, &faces(&[(700, true)])).unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].file_name, "RobotoSlab[wght].ttf");
}

#[test]
fn unsafe_file_names_are_never_planned() {
    let text = metadata_static(
        "Evil",
        "OFL",
        &[("../../x.ttf", "normal", 400), ("a/b.ttf", "normal", 700)],
    );
    assert!(plan_files(&parse_metadata(&text), &BTreeSet::new()).is_none());
}
