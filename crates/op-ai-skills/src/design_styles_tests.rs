//! Invariants of the style definitions table (`data/design-styles-uiuxpm.json`,
//! written by `scripts/import-ui-ux-pro-max.py`). They read the committed
//! files only: no network, no upstream CSV (the regeneration check is
//! `import_is_reproducible_from_pinned_source` in the import tests, which
//! covers both files when `UIUXPM_SRC` is set).

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::*;
use crate::design_data::{
    design_records, load_all_tables, parse_design_data, DESIGN_DATA_JSON, IMPORTED_DATA_JSON,
    STYLES_DATA_JSON,
};
use crate::design_styles_search::{search_styles, styles_to_json, StyleHit};

/// Style records the generator writes for the pinned commit: 89 upstream rows
/// minus 9 deprecated and the 3 Apple-derived ones. Change it together with
/// the generator and the regenerated file.
const EXPECTED_STYLE_RECORDS: usize = 77;
const APPLE_STYLE_REFS: [&str; 3] = [
    "liquid-glass",
    "apple-native-experience",
    "spatial-ui-visionos",
];
/// `external:` names allowed to have no style record. None: the generator
/// removes such a name from the product record instead.
const ALLOWED_UNRESOLVED_EXTERNAL_NAMES: [&str; 0] = [];

fn styles_value() -> Value {
    serde_json::from_str(STYLES_DATA_JSON).expect("styles JSON is valid")
}

fn embedded_styles() -> Vec<StyleRecord> {
    parse_style_data(STYLES_DATA_JSON).expect("the styles file validates")
}

fn rejected_field(raw: &str) -> String {
    match parse_style_data(raw) {
        Err(DesignDataError::Record { field, .. }) => field,
        Err(other) => other.to_string(),
        Ok(_) => panic!("expected a rejection"),
    }
}

#[test]
fn style_records_validate_and_have_provenance() {
    let styles = embedded_styles();
    assert_eq!(styles.len(), EXPECTED_STYLE_RECORDS);
    let ids: Vec<&str> = styles.iter().map(|style| style.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "records are sorted by id");
    assert!(STYLES_DATA_JSON.ends_with("]\n"));
    for style in &styles {
        assert_eq!(style.id, format!("{STYLE_ID_PREFIX}{}", style.reference));
        assert_eq!(style.source.name, "UI UX Pro Max (MIT)", "{}", style.id);
        assert_eq!(style.source.license_spdx, "MIT", "{}", style.id);
        assert_eq!(
            style.source.url,
            "https://github.com/nextlevelbuilder/ui-ux-pro-max-skill"
        );
        assert_eq!(style.source.commit.len(), 40, "{}", style.id);
        assert!(style.source.commit.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(style.source.retrieved, "2026-11-18");
        assert!(style.keywords.len() <= 20, "{}", style.id);
        assert!(style.best_for.is_some(), "{} has no bestFor", style.id);
        assert_ne!(style.status.as_deref(), Some("deprecated"), "{}", style.id);
        for color in style.primary_colors.iter().chain(&style.secondary_colors) {
            assert!(is_hex_color(color) && color == &color.to_ascii_lowercase());
        }
    }
    // One bad record rejects the WHOLE file, never just itself.
    let broken = |change: &dyn Fn(&mut Value)| -> String {
        let mut value = styles_value();
        change(&mut value[5]);
        value.to_string()
    };
    for (label, raw, field) in [
        (
            "no source",
            broken(&|r| drop(r.as_object_mut().unwrap().remove("source"))),
            "source",
        ),
        (
            "no ref",
            broken(&|r| drop(r.as_object_mut().unwrap().remove("ref"))),
            "ref",
        ),
        (
            "ref and id disagree",
            broken(&|r| r["ref"] = json!("other")),
            "ref",
        ),
        (
            "id outside the namespace",
            broken(&|r| r["id"] = json!("minimalism")),
            "id",
        ),
        (
            "upper-case keyword",
            broken(&|r| r["keywords"] = json!(["Loud"])),
            "keywords",
        ),
        (
            "no keywords",
            broken(&|r| r["keywords"] = json!([])),
            "keywords",
        ),
        (
            "control character",
            broken(&|r| r["bestFor"] = json!("a\u{7}b")),
            "bestFor",
        ),
        (
            "empty text",
            broken(&|r| r["effects"] = json!("")),
            "effects",
        ),
        (
            "prose over the cap",
            broken(&|r| r["effects"] = json!("x".repeat(401))),
            "effects",
        ),
        (
            "non-hex colour",
            broken(&|r| r["primaryColors"] = json!(["red"])),
            "primaryColors",
        ),
        (
            "licence",
            broken(&|r| r["source"]["licenseSpdx"] = json!("MIT; rm")),
            "source.licenseSpdx",
        ),
    ] {
        assert_eq!(rejected_field(&raw), field, "{label}");
    }
    let mut duplicate = styles_value();
    duplicate[1]["ref"] = duplicate[0]["ref"].clone();
    duplicate[1]["id"] = duplicate[0]["id"].clone();
    assert!(matches!(
        parse_style_data(&duplicate.to_string()),
        Err(DesignDataError::Record {
            problem: FieldProblem::DuplicateId,
            ..
        })
    ));
    assert!(matches!(
        parse_style_data("not json"),
        Err(DesignDataError::Json(_))
    ));
    assert!(matches!(
        parse_style_data("{}"),
        Err(DesignDataError::NotAnArray)
    ));
    let oversized = format!("[\"{}\"]", "a".repeat(MAX_DATA_BYTES));
    assert!(matches!(
        parse_style_data(&oversized),
        Err(DesignDataError::TooLarge { .. })
    ));
}

fn external_names() -> Vec<(String, String)> {
    design_records()
        .iter()
        .flat_map(|record| {
            record
                .styles
                .iter()
                .filter_map(|style| style.strip_prefix(EXTERNAL_STYLE_PREFIX))
                .map(|name| (record.id.clone(), name.to_string()))
        })
        .collect()
}

#[test]
fn every_resolved_style_ref_has_exactly_one_record() {
    let styles = embedded_styles();
    let refs: BTreeSet<&str> = styles
        .iter()
        .map(|style| style.reference.as_str())
        .collect();
    assert_eq!(refs.len(), styles.len(), "a ref is used by one record only");
    let names = external_names();
    assert!(
        names.len() > 600,
        "the product file cites many styles: {}",
        names.len()
    );
    for (record_id, name) in &names {
        let records: Vec<&StyleRecord> = styles
            .iter()
            .filter(|style| &style.reference == name)
            .collect();
        assert_eq!(records.len(), 1, "{record_id}: external:{name}");
        // Every spelling an agent may use finds that same record.
        for spelling in [
            name.clone(),
            format!("external:{name}"),
            records[0].id.clone(),
            records[0].name.to_uppercase(),
        ] {
            assert_eq!(
                style_by_ref(&spelling).map(|found| &found.id),
                Some(&records[0].id),
                "{spelling:?}"
            );
        }
    }
}

#[test]
fn unresolved_external_names_are_not_left_in_product_records() {
    let unresolved: BTreeSet<String> = external_names()
        .into_iter()
        .filter(|(_, name)| style_by_ref(name).is_none())
        .map(|(_, name)| name)
        .filter(|name| !ALLOWED_UNRESOLVED_EXTERNAL_NAMES.contains(&name.as_str()))
        .collect();
    assert!(unresolved.is_empty(), "no style record for {unresolved:?}");
}

#[test]
fn no_apple_derived_style_survives() {
    let lower = STYLES_DATA_JSON.to_lowercase();
    for forbidden in [
        "liquid-glass",
        "liquid glass",
        "apple-native-experience",
        "apple",
        "human interface",
        "visionos",
        "spatial-ui",
    ] {
        assert!(
            !lower.contains(forbidden),
            "styles data mentions {forbidden:?}"
        );
    }
    assert!(
        !STYLES_DATA_JSON
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "HIG"),
        "styles data mentions HIG"
    );
    for reference in APPLE_STYLE_REFS {
        assert!(style_by_ref(reference).is_none(), "{reference}");
        assert!(style_by_ref(&format!("external:{reference}")).is_none());
    }
    for query in [
        "liquid glass",
        "visionos spatial ui",
        "apple native experience",
    ] {
        let hits = search_styles(query, 3);
        assert!(
            hits.iter()
                .all(|hit| !APPLE_STYLE_REFS.contains(&hit.record.reference.as_str())),
            "{query}"
        );
    }
    // No product record cites one either.
    for (record, name) in external_names() {
        assert!(
            !APPLE_STYLE_REFS.contains(&name.as_str()),
            "{record}: {name}"
        );
    }
}

#[test]
fn bad_styles_file_does_not_reject_the_product_tables() {
    let seed_count = parse_design_data(DESIGN_DATA_JSON).expect("seed").len();
    let imported_count = parse_design_data(IMPORTED_DATA_JSON)
        .expect("imported")
        .len();
    let good = styles_value();
    let with = |change: &dyn Fn(&mut Vec<Value>)| -> String {
        let mut records = good.as_array().expect("array").clone();
        change(&mut records);
        Value::Array(records).to_string()
    };
    let bad_files = [
        ("not json", "not json".to_string()),
        ("not an array", "{}".to_string()),
        (
            "one invalid record",
            with(&|r| r[7]["primaryColors"] = json!(["red"])),
        ),
        (
            "a duplicate ref",
            with(&|r| r[1]["ref"] = r[0]["ref"].clone()),
        ),
        ("oversized", format!("[\"{}\"]", "a".repeat(MAX_DATA_BYTES))),
    ];
    for (label, bad) in &bad_files {
        let (tables, problems) = load_all_tables(DESIGN_DATA_JSON, IMPORTED_DATA_JSON, bad);
        assert_eq!(tables.records.len(), seed_count + imported_count, "{label}");
        assert!(
            tables.styles.is_empty(),
            "{label}: the bad file is skipped whole"
        );
        assert_eq!(problems.len(), 1, "{label}: one log line, got {problems:?}");
        assert!(problems[0].starts_with("style table disabled"), "{label}");
    }
    // The other way round: bad product files never take the styles with them.
    let (tables, problems) = load_all_tables("[{}]", "not json", STYLES_DATA_JSON);
    assert!(tables.records.is_empty());
    assert_eq!(tables.styles.len(), EXPECTED_STYLE_RECORDS);
    assert_eq!(problems.len(), 2, "{problems:?}");
    // All good: no log line.
    let (tables, problems) =
        load_all_tables(DESIGN_DATA_JSON, IMPORTED_DATA_JSON, STYLES_DATA_JSON);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tables.styles.len(), EXPECTED_STYLE_RECORDS);
}

#[test]
fn style_file_is_native_only_and_under_the_ceiling() {
    // The embed sits behind `cfg(not(wasm32))` and wasm32 gets an empty table.
    let source = include_str!("design_data.rs");
    let embed = r#"#[cfg(not(target_arch = "wasm32"))]
pub(crate) const STYLES_DATA_JSON: &str = include_str!("../data/design-styles-uiuxpm.json");
#[cfg(target_arch = "wasm32")]
pub(crate) const STYLES_DATA_JSON: &str = "[]";"#;
    assert!(
        source.contains(embed),
        "the styles embed must be cfg-gated off wasm32"
    );
    assert_eq!(
        source.matches("design-styles-uiuxpm.json\")").count(),
        1,
        "the file is embedded in exactly one place"
    );
    assert!(
        STYLES_DATA_JSON.len() < MAX_DATA_BYTES / 2,
        "{} bytes",
        STYLES_DATA_JSON.len()
    );
    // Every style fits one response without losing a field.
    for record in style_records() {
        let hit = StyleHit { record, score: 10 };
        let text = styles_to_json(&[hit]);
        assert!(
            text.len() <= crate::design_data_search::MAX_RESPONSE_BYTES,
            "{}",
            record.id
        );
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            value["matches"][0]["effects"].as_str(),
            record.effects.as_deref()
        );
    }
}
