//! Invariants of the imported UI UX Pro Max table
//! (`data/design-data-uiuxpm.json`, written by
//! `scripts/import-ui-ux-pro-max.py`). They read the committed file only: no
//! network, no upstream CSV. The regeneration check runs when the pinned CSV
//! directory is supplied through `UIUXPM_SRC`.

use super::*;
use crate::design_data_search::{
    design_data_json, hits_to_json, search_design_data, search_records, MAX_RESPONSE_BYTES,
};
use serde_json::json;

/// Records the generator writes for the pinned commit: 192 upstream product
/// types minus the visionOS-style "Spatial Computing OS / App". (19 `rgba(...)`
/// borders are composited over their Background, none is skipped.) Change it
/// together with the generator and the regenerated file.
const EXPECTED_IMPORTED_RECORDS: usize = 191;
const SEED_SOURCE: &str = "OpenPencil seed";
const IMPORT_SOURCE: &str = "UI UX Pro Max (MIT)";

fn imported_value() -> Value {
    serde_json::from_str(IMPORTED_DATA_JSON).expect("imported JSON is valid")
}

fn imported_records() -> Vec<DesignRecord> {
    parse_design_data(IMPORTED_DATA_JSON).expect("the imported file validates")
}

fn ids(hits: &[crate::design_data_search::DesignHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.record.id.as_str()).collect()
}

fn leak(records: Vec<DesignRecord>) -> &'static [DesignRecord] {
    Box::leak(records.into_boxed_slice())
}

#[test]
fn import_is_reproducible_from_pinned_source() {
    // Offline half: the committed file is exactly what one generator run on
    // the pinned commit produces - right count, sorted, one commit, one date.
    let records = imported_records();
    assert_eq!(records.len(), EXPECTED_IMPORTED_RECORDS);
    let mut sorted: Vec<&str> = records.iter().map(|r| r.id.as_str()).collect();
    let before = sorted.clone();
    sorted.sort_unstable();
    assert_eq!(before, sorted, "records are sorted by id");
    let commit = &records[0].source.commit;
    assert_eq!(commit.len(), 40, "a full commit SHA");
    assert!(records
        .iter()
        .all(|r| &r.source.commit == commit && r.source.retrieved == records[0].source.retrieved));
    // Output format is the generator's: 2-space indent, trailing newline.
    assert!(IMPORTED_DATA_JSON.ends_with("]\n"));
    assert!(IMPORTED_DATA_JSON.starts_with("[\n  {\n    \"id\""));

    // Online half, only when the pinned CSV directory is supplied:
    // UIUXPM_SRC=<dir> cargo test -p op-ai-skills import_is_reproducible
    let Ok(source_dir) = std::env::var("UIUXPM_SRC") else {
        return;
    };
    if !std::path::Path::new(&source_dir).is_dir() {
        return;
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/import-ui-ux-pro-max.py");
    let output = std::process::Command::new("python3")
        .arg("-I")
        .arg(&script)
        .args(["--check", "--src", &source_dir, "--retrieved"])
        .arg(&records[0].source.retrieved)
        .output()
        .expect("run python3");
    assert!(
        output.status.success(),
        "generator --check failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn imported_records_all_validate_and_have_provenance() {
    let known_styles: Vec<&str> = crate::style_guide::loader::style_guide_registry()
        .iter()
        .map(|guide| guide.name.as_str())
        .collect();
    for record in imported_records() {
        assert!(record.id.starts_with(IMPORTED_ID_PREFIX), "{}", record.id);
        assert_eq!(record.source.name, IMPORT_SOURCE, "{}", record.id);
        assert_eq!(record.source.license_spdx, "MIT", "{}", record.id);
        assert_eq!(
            record.source.url,
            "https://github.com/nextlevelbuilder/ui-ux-pro-max-skill"
        );
        assert!(
            record.source.commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "{}",
            record.id
        );
        assert!(is_iso_date(&record.source.retrieved));
        assert!(record.platforms.contains(&"web".to_string()));
        assert!(!record.typography.heading.is_empty() && !record.typography.body.is_empty());
        for style in &record.styles {
            assert!(
                style.starts_with(EXTERNAL_STYLE_PREFIX) || known_styles.contains(&style.as_str()),
                "{}: style {style:?}",
                record.id
            );
        }
        // Fonts are names, never URLs or CSS.
        for font in [&record.typography.heading, &record.typography.body] {
            assert!(
                !font.contains(['/', ':', '{', ';']),
                "{}: {font}",
                record.id
            );
        }
    }
    // No seed id sits in the imported namespace.
    for record in design_records()
        .iter()
        .filter(|r| r.source.name == SEED_SOURCE)
    {
        assert!(!record.id.starts_with(IMPORTED_ID_PREFIX), "{}", record.id);
    }
}

#[test]
fn imported_palettes_are_valid_hex_and_complete() {
    let roles = [
        "primary",
        "onPrimary",
        "secondary",
        "accent",
        "background",
        "foreground",
        "card",
        "muted",
        "border",
        "destructive",
    ];
    for record in imported_value().as_array().expect("array") {
        let palette = record["palette"].as_object().expect("palette");
        assert_eq!(palette.len(), roles.len(), "{}", record["id"]);
        for role in roles {
            let hex = palette[role].as_str().expect("hex string");
            assert!(
                hex.len() == 7
                    && hex.starts_with('#')
                    && hex[1..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{} {role} {hex}",
                record["id"]
            );
        }
    }
}

#[test]
fn no_apple_sourced_style_survives() {
    let lower = IMPORTED_DATA_JSON.to_lowercase();
    for forbidden in [
        "liquid-glass",
        "liquid glass",
        "apple-native-experience",
        "apple native",
        "apple",
        "human interface",
        "visionos",
        "spatial-ui",
    ] {
        assert!(
            !lower.contains(forbidden),
            "imported data mentions {forbidden:?}"
        );
    }
    // `HIG` as a word, case-sensitive: "high" is everywhere.
    assert!(
        !IMPORTED_DATA_JSON
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == "HIG"),
        "imported data mentions HIG"
    );
}

#[test]
fn no_instruction_like_text_in_imported_records() {
    let lower = IMPORTED_DATA_JSON.to_lowercase();
    for phrase in [
        "ignore previous",
        "ignore all previous",
        "ignore any previous",
        "ignore prior",
        "ignore all prior",
        "ignore any prior",
        "system prompt",
        "you are now",
        "disregard",
        "<system",
        "tool_use",
    ] {
        assert!(!lower.contains(phrase), "instruction-like text {phrase:?}");
    }
    // No invisible or control characters (newline is the only allowed one).
    assert!(
        !IMPORTED_DATA_JSON
            .chars()
            .any(|c| (c.is_control() && c != '\n')
                || ('\u{200b}'..='\u{200f}').contains(&c)
                || ('\u{202a}'..='\u{202e}').contains(&c)
                || ('\u{2066}'..='\u{2069}').contains(&c)),
        "control or bidi character in the imported file"
    );
}

#[test]
fn bad_imported_file_does_not_reject_the_seed() {
    let seed_count = parse_design_data(DESIGN_DATA_JSON).expect("seed").len();
    let imported = imported_value();
    let with = |change: &dyn Fn(&mut Vec<Value>)| -> String {
        let mut records = imported.as_array().expect("array").clone();
        change(&mut records);
        Value::Array(records).to_string()
    };
    let bad_files: Vec<(&str, String)> = vec![
        ("not json", "not json".to_string()),
        ("not an array", "{}".to_string()),
        (
            "one invalid record",
            with(&|r| r[7]["palette"]["primary"] = json!("red")),
        ),
        (
            "an id outside the namespace",
            with(&|r| r[3]["id"] = json!("fintech-dashboard")),
        ),
        (
            "a duplicate id inside the file",
            with(&|r| r[1]["id"] = r[0]["id"].clone()),
        ),
        ("oversized", format!("[\"{}\"]", "a".repeat(MAX_DATA_BYTES))),
    ];
    for (label, bad) in &bad_files {
        let (records, problems) = load_tables(DESIGN_DATA_JSON, bad);
        assert_eq!(records.len(), seed_count, "{label}: only the seed loads");
        assert!(
            records.iter().all(|r| r.source.name == SEED_SOURCE),
            "{label}"
        );
        assert_eq!(problems.len(), 1, "{label}: one log line, got {problems:?}");
        assert!(
            problems[0].starts_with("imported table disabled"),
            "{label}"
        );
    }
    // The rule inside one file is unchanged: a single bad record drops that
    // whole file, not just the record.
    let (records, _) = load_tables("[]", &bad_files[2].1);
    assert!(records.is_empty());

    // Crossing the two files: an imported id that repeats a seed id, and a
    // union above the record ceiling, also skip only the imported file.
    let mut clashing_seed: Value = serde_json::from_str(DESIGN_DATA_JSON).expect("seed");
    clashing_seed[0]["id"] = imported[0]["id"].clone();
    let (records, problems) = load_tables(&clashing_seed.to_string(), IMPORTED_DATA_JSON);
    assert_eq!(records.len(), seed_count);
    assert!(
        problems[0].contains("duplicates an earlier record"),
        "{problems:?}"
    );
    let template = imported[0].clone();
    let crowded: Vec<Value> = (0..MAX_RECORDS)
        .map(|n| {
            let mut record = template.clone();
            record["id"] = json!(format!("{IMPORTED_ID_PREFIX}crowd-{n}"));
            record
        })
        .collect();
    let (records, problems) = load_tables(DESIGN_DATA_JSON, &Value::Array(crowded).to_string());
    assert_eq!(records.len(), seed_count);
    assert!(problems[0].contains("limit"), "{problems:?}");

    // And the other way round: a bad seed does not take the import with it.
    let (records, problems) = load_tables("[{}]", IMPORTED_DATA_JSON);
    assert_eq!(records.len(), EXPECTED_IMPORTED_RECORDS);
    assert!(
        problems[0].starts_with("seed table disabled"),
        "{problems:?}"
    );

    // Both good: the union, no log line.
    let (records, problems) = load_tables(DESIGN_DATA_JSON, IMPORTED_DATA_JSON);
    assert!(problems.is_empty());
    assert_eq!(records.len(), seed_count + EXPECTED_IMPORTED_RECORDS);
}

#[test]
fn ids_are_unique_across_files() {
    let mut seen = std::collections::BTreeSet::new();
    for record in design_records() {
        assert!(
            seen.insert(record.id.as_str()),
            "duplicate id {}",
            record.id
        );
    }
    let seed = parse_design_data(DESIGN_DATA_JSON).expect("seed");
    for record in imported_records() {
        assert!(seed.iter().all(|s| s.id != record.id), "{}", record.id);
    }
    assert!(design_records().len() <= MAX_RECORDS);
}

#[test]
fn search_finds_imported_product_types() {
    let top = |query: &str, n: usize| -> Vec<String> {
        ids(&search_design_data(query, None, n))
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    // Exact product types from the imported table.
    assert_eq!(
        top("restaurant menu", 1),
        ["uiuxpm-restaurant-food-service"]
    );
    assert_eq!(top("saas general", 1), ["uiuxpm-saas-general"]);
    assert_eq!(top("gaming", 1), ["uiuxpm-gaming"]);
    assert_eq!(
        top("children education mobile app", 1),
        ["uiuxpm-kids-learning-abc-math"]
    );
    // Close neighbours show up in the first three.
    for (query, expected) in [
        ("yoga studio booking", "uiuxpm-yoga-stretching-guide"),
        ("luxury hotel", "uiuxpm-hotel-hospitality"),
        ("fintech dashboard", "uiuxpm-financial-dashboard"),
        (
            "healthcare patient portal",
            "uiuxpm-patient-portal-health-records",
        ),
        ("travel booking", "uiuxpm-travel-tourism-agency"),
    ] {
        assert!(
            top(query, 3).iter().any(|id| id == expected),
            "{query:?} -> {:?}, expected {expected}",
            top(query, 3)
        );
    }
    // The seed still wins where its own product type is named.
    assert_eq!(top("fintech dashboard", 1), ["fintech-dashboard"]);
    assert_eq!(top("saas landing page", 1), ["saas-landing"]);
    // The platform filter sees imported platforms.
    let mobile = search_design_data("children education", Some("mobile"), 3);
    assert!(!mobile.is_empty());
    assert!(mobile
        .iter()
        .all(|hit| hit.record.platforms.contains(&"mobile".to_string())));
}

#[test]
fn an_exact_product_type_wins_over_a_higher_keyword_score_in_either_file() {
    let imported = imported_records();
    let base = imported[0].clone();
    let mut exact = base.clone();
    exact.id = format!("{IMPORTED_ID_PREFIX}zebra-shop");
    exact.name = "Zebra Shop".to_string();
    exact.keywords = vec!["zebra shop".to_string()];
    let mut keyword_heavy = base.clone();
    keyword_heavy.id = "aaa-many-keywords".to_string();
    keyword_heavy.name = "Unrelated Name".to_string();
    keyword_heavy.keywords = ["zebra", "shop", "stripes", "savanna", "plains", "mane"]
        .map(str::to_string)
        .to_vec();
    let records = leak(vec![keyword_heavy, exact]);
    let hits = search_records(records, "zebra shop stripes savanna plains mane", None, 2);
    assert_eq!(hits.len(), 2);
    assert!(
        hits[1].score > hits[0].score,
        "the exact record must win on the tier, not on score"
    );
    assert_eq!(
        ids(&hits)[0],
        "uiuxpm-zebra-shop",
        "the exact product type ranks first even though the other record scores {}",
        hits[1].score
    );
}

#[test]
fn weak_query_still_returns_empty() {
    for query in [
        "",
        "   ",
        "app",
        "apps",
        "platform",
        "general",
        "web",
        "website",
        "design",
        "a nice page",
        "hello world",
        "x",
        "formal letter",
        "sidebar table cards",
        "hero headline",
    ] {
        let hits = search_design_data(query, None, 3);
        assert!(hits.is_empty(), "{query:?} returned {:?}", ids(&hits));
    }
    let message: Value =
        serde_json::from_str(&design_data_json("platform", None, 3)).expect("json");
    assert_eq!(message["matches"], json!([]));
}

#[test]
fn no_imported_keyword_is_a_generic_word() {
    // A single keyword hit already reaches the match threshold, so a filler
    // word as a keyword would make every query that contains it a match.
    for record in imported_records() {
        for keyword in &record.keywords {
            assert!(
                ![
                    "app", "apps", "platform", "general", "web", "website", "site", "page",
                    "design"
                ]
                .contains(&keyword.as_str()),
                "{}: keyword {keyword:?}",
                record.id
            );
        }
    }
}

#[test]
fn file_sizes_are_under_the_ceiling() {
    for (label, raw) in [("seed", DESIGN_DATA_JSON), ("imported", IMPORTED_DATA_JSON)] {
        assert!(raw.len() < MAX_DATA_BYTES, "{label} is {} bytes", raw.len());
    }
    assert!(
        IMPORTED_DATA_JSON.len() < MAX_DATA_BYTES / 2,
        "the imported file is {} bytes: half the ceiling is the review trigger",
        IMPORTED_DATA_JSON.len()
    );
    assert!(design_records().len() <= MAX_RECORDS);
    // Every record fits one get_guidelines response without being cut.
    for record in design_records() {
        let text = hits_to_json(&[crate::design_data_search::DesignHit { record, score: 10 }]);
        assert!(
            text.len() <= MAX_RESPONSE_BYTES,
            "{}: {} bytes",
            record.id,
            text.len()
        );
        let value: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(
            value["matches"][0]["rules"].as_array().map(Vec::len),
            Some(record.rules.len()),
            "{}: rules were trimmed to fit",
            record.id
        );
    }
}

/// `rgba(r,g,b,alpha)` over an opaque `#rrggbb`, per channel
/// round(alpha*fg + (1-alpha)*bg) - the rule in the generator header.
fn composite(foreground: [u8; 3], alpha: f64, background: &str) -> String {
    let mut out = String::from("#");
    for (index, fg) in foreground.iter().enumerate() {
        let bg = u8::from_str_radix(&background[1 + 2 * index..3 + 2 * index], 16).expect("hex");
        let mixed = (alpha * f64::from(*fg) + (1.0 - alpha) * f64::from(bg) + 0.5) as u8;
        out.push_str(&format!("{mixed:02x}"));
    }
    out
}

#[test]
fn rgba_border_is_composited_over_background() {
    // Table: (rgba foreground, alpha, background, expected).
    for (foreground, alpha, background, expected) in [
        ([255, 255, 255], 0.08, "#0f172a", "#222a3b"),
        ([255, 255, 255], 0.08, "#000000", "#141414"),
        ([255, 255, 255], 1.0, "#0f172a", "#ffffff"),
        ([255, 255, 255], 0.0, "#0f172a", "#0f172a"),
        ([10, 20, 30], 1.0, "#ffffff", "#0a141e"),
    ] {
        assert_eq!(composite(foreground, alpha, background), expected);
    }
    // The generator implements the same table (python3 -I, no network).
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/import-ui-ux-pro-max.py");
    let probe = format!(
        "import importlib.util as u;s=u.spec_from_file_location('m',{script:?});m=u.module_from_spec(s);\
         s.loader.exec_module(m);c=m.composite_over;\
         print(c('rgba(255,255,255,0.08)','#0f172a'),c('rgba(255 255 255 / 0.08)','#000000'),\
         c('rgba(255,255,255,1)','#0f172a'),c('rgba(255,255,255,0)','#0f172a'),c('red','#000000'))"
    );
    if let Ok(output) = std::process::Command::new("python3")
        .args(["-I", "-c", &probe])
        .output()
    {
        if output.status.success() {
            assert_eq!(
                String::from_utf8_lossy(&output.stdout).trim(),
                "#222a3b #141414 #ffffff #0f172a None"
            );
        }
    }
    // The committed data: the 19 upstream white-at-8% borders are the
    // composited colour, a little lighter than their own background.
    let mut composited = 0;
    for record in imported_records() {
        let palette = &record.palette;
        let expected = composite([255, 255, 255], 0.08, &palette.background);
        if palette.border == expected {
            composited += 1;
            assert_ne!(palette.border, palette.background, "{}", record.id);
        }
    }
    assert!(composited >= 19, "only {composited} borders match the rule");
    let finance = imported_records()
        .into_iter()
        .find(|record| record.id == "uiuxpm-personal-finance-tracker")
        .expect("recovered record is present");
    assert_eq!(
        finance.palette.border,
        composite([255, 255, 255], 0.08, &finance.palette.background)
    );
}

#[test]
fn apple_platform_ui_records_are_not_imported() {
    assert!(imported_records()
        .iter()
        .all(|record| record.id != "uiuxpm-spatial-computing-os-app"));
    for record in imported_records() {
        let subject = format!("{} {}", record.name, record.keywords.join(" ")).to_lowercase();
        for word in ["visionos", "vision pro", "watchos", "ipados", "macos"] {
            assert!(!subject.contains(word), "{}: {word}", record.id);
        }
    }
}
