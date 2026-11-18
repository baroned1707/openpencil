use super::*;
use crate::color::wcag;
use crate::style_guide::loader::style_guide_registry;
use serde_json::json;

fn is_seed(record: &DesignRecord) -> bool {
    record.source.name == "OpenPencil seed"
}

fn seed_value() -> Value {
    serde_json::from_str(DESIGN_DATA_JSON).expect("seed JSON is valid")
}

fn seed_record(index: usize) -> Value {
    seed_value()[index].clone()
}

fn parse_one(record: Value) -> Result<Vec<DesignRecord>, DesignDataError> {
    parse_design_data(&json!([record]).to_string())
}

fn rejected_field(result: Result<Vec<DesignRecord>, DesignDataError>) -> String {
    match result {
        Err(DesignDataError::Record { field, .. }) => field,
        other => panic!("expected a record error, got {other:?}"),
    }
}

#[test]
fn seed_records_parse_and_validate() {
    let records = design_records();
    let seeds = records.iter().filter(|record| is_seed(record)).count();
    assert!(seeds >= 6, "seed table has {seeds} records");
    let mut ids: Vec<&str> = records.iter().map(|record| record.id.as_str()).collect();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "ids must be unique");
    for expected in [
        "fintech-dashboard",
        "saas-landing",
        "mobile-health-tracker",
        "ecommerce-storefront",
        "developer-docs",
        "portfolio-creative",
    ] {
        assert!(ids.contains(&expected), "seed record {expected} missing");
    }

    let known_styles: Vec<&str> = style_guide_registry()
        .iter()
        .map(|guide| guide.name.as_str())
        .collect();
    for record in records {
        assert!(is_slug(&record.id), "{}", record.id);
        for keyword in &record.keywords {
            assert!(
                !keyword.chars().any(|c| c.is_uppercase()),
                "{}: keyword {keyword:?} is not lower-case",
                record.id
            );
        }
        for (role, hex) in record.palette.entries() {
            assert!(is_hex_color(hex), "{} {role} {hex}", record.id);
        }
        for style in &record.styles {
            assert!(
                style.starts_with(EXTERNAL_STYLE_PREFIX) || known_styles.contains(&style.as_str()),
                "{}: style {style:?} is neither an embedded style guide nor external:",
                record.id
            );
        }
        assert!(!record.source.name.is_empty() && !record.source.license_spdx.is_empty());
        assert!(is_iso_date(&record.source.retrieved));
        if record.source.name == "OpenPencil seed" {
            assert_eq!(record.source.license_spdx, "MIT");
        }
    }
}

#[test]
fn palette_pairs_meet_wcag_aa() {
    // The seed is hand-written to pass; the imported palettes are upstream
    // data and are only checked for being complete valid hex.
    for record in design_records().iter().filter(|record| is_seed(record)) {
        let palette = &record.palette;
        for (label, foreground, background) in [
            (
                "foreground/background",
                &palette.foreground,
                &palette.background,
            ),
            ("onPrimary/primary", &palette.on_primary, &palette.primary),
        ] {
            let ratio = wcag(foreground, background).expect("valid hex pair");
            assert!(
                ratio >= 4.5,
                "{} {label} contrast {ratio:.2} is below WCAG AA 4.5",
                record.id
            );
        }
    }
}

#[test]
fn loader_rejects_oversized_and_control_chars() {
    // Size ceiling: rejected before parsing.
    let huge = format!("[\"{}\"]", "a".repeat(MAX_DATA_BYTES));
    assert!(matches!(
        parse_design_data(&huge),
        Err(DesignDataError::TooLarge { .. })
    ));

    // Control characters (other than newline) in any text field.
    for (field, control) in [
        ("pattern", "\u{0007}"),
        ("name", "\u{001b}[31m"),
        ("notes", "\u{0000}"),
    ] {
        let mut record = seed_record(0);
        record[field] = json!(format!("fine {control} text"));
        assert_eq!(rejected_field(parse_one(record)), field);
    }
    let mut with_newline = seed_record(0);
    with_newline["notes"] = json!("two\nlines are fine");
    assert!(parse_one(with_newline).is_ok());

    // Per-field limits.
    let too_long = |n: usize| "x".repeat(n);
    for (field, limit) in [
        ("name", MAX_NAME_CHARS),
        ("pattern", MAX_PATTERN_CHARS),
        ("notes", MAX_NOTES_CHARS),
    ] {
        let mut ok = seed_record(0);
        ok[field] = json!(too_long(limit));
        assert!(parse_one(ok).is_ok(), "{field} at the limit is accepted");
        let mut bad = seed_record(0);
        bad[field] = json!(too_long(limit + 1));
        assert_eq!(rejected_field(parse_one(bad)), field);
    }
    let mut long_rule = seed_record(0);
    long_rule["rules"][0] = json!({ "when": too_long(200), "do": too_long(101) });
    assert_eq!(rejected_field(parse_one(long_rule)), "rules");
}

#[test]
fn loader_rejects_malformed_records_as_a_whole_table() {
    let good = seed_record(0);
    type Mutation = Box<dyn Fn(&mut Value)>;
    let mutations: Vec<(&str, Mutation)> = vec![
        ("id", Box::new(|r| r["id"] = json!("Bad Id"))),
        ("keywords", Box::new(|r| r["keywords"] = json!(["Fintech"]))),
        ("keywords", Box::new(|r| r["keywords"] = json!([]))),
        (
            "platforms",
            Box::new(|r| r["platforms"] = json!(["fridge"])),
        ),
        (
            "primary",
            Box::new(|r| r["palette"]["primary"] = json!("#fff")),
        ),
        (
            "accent",
            Box::new(|r| r["palette"]["accent"] = json!("#12345g")),
        ),
        (
            "destructive",
            Box::new(|r| r["palette"]["destructive"] = json!("red")),
        ),
        (
            "source",
            Box::new(|r| {
                r.as_object_mut().expect("object").remove("source");
            }),
        ),
        (
            "source.url",
            Box::new(|r| r["source"]["url"] = json!("http://insecure.example")),
        ),
        (
            "source.licenseSpdx",
            Box::new(|r| r["source"]["licenseSpdx"] = json!("MIT; rm -rf")),
        ),
        (
            "source.retrieved",
            Box::new(|r| r["source"]["retrieved"] = json!("yesterday")),
        ),
        ("rules", Box::new(|r| r["rules"] = json!([]))),
        ("typography", Box::new(|r| r["typography"] = json!("Inter"))),
    ];
    for (expected_field, mutate) in mutations {
        let mut record = good.clone();
        mutate(&mut record);
        // A valid record next to it must not rescue the table.
        let table = json!([seed_record(1), record]).to_string();
        match parse_design_data(&table) {
            Err(DesignDataError::Record { field, .. }) => assert_eq!(field, expected_field),
            other => panic!("{expected_field}: malformed record accepted: {other:?}"),
        }
    }
    let duplicate = json!([good.clone(), good]).to_string();
    assert!(matches!(
        parse_design_data(&duplicate),
        Err(DesignDataError::Record {
            problem: FieldProblem::DuplicateId,
            ..
        })
    ));
    assert!(matches!(
        parse_design_data("{}"),
        Err(DesignDataError::NotAnArray)
    ));
    assert!(matches!(
        parse_design_data("not json"),
        Err(DesignDataError::Json(_))
    ));
}

#[test]
fn a_bad_table_loads_as_empty_not_a_panic() {
    // The cache wrapper maps any error to an empty table; the parse step it
    // wraps never panics on hostile input.
    for hostile in ["", "[", "[null]", "[{}]", "[[]]", "\u{0000}", "[1,2,3]"] {
        assert!(parse_design_data(hostile).is_err(), "{hostile:?}");
    }
}
