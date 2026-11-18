//! Attribution gate: every data source other than the project's own seed must
//! have its licence section in `packaging/shared/THIRD_PARTY_NOTICES.md`, so
//! the importer of a new dataset cannot ship without attribution.

use std::collections::BTreeSet;
use std::path::Path;

use super::design_records;

const SEED_SOURCE: &str = "OpenPencil seed";

fn notices_markdown() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/shared/THIRD_PARTY_NOTICES.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// Text of every level-2 heading outside HTML comments.
fn section_headings(markdown: &str) -> BTreeSet<String> {
    let mut visible = String::new();
    let mut rest = markdown;
    while let Some(start) = rest.find("<!--") {
        visible.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    visible.push_str(rest);
    visible
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .map(|heading| heading.trim().to_string())
        .collect()
}

/// Source names that need a section and do not have one.
fn missing_sections<'a>(
    source_names: impl IntoIterator<Item = &'a str>,
    markdown: &str,
) -> Vec<String> {
    let headings = section_headings(markdown);
    let needed: BTreeSet<&str> = source_names
        .into_iter()
        .filter(|name| *name != SEED_SOURCE)
        .collect();
    needed
        .into_iter()
        .filter(|name| !headings.contains(*name))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_non_seed_source_has_a_notice_section() {
    let markdown = notices_markdown();
    let names: Vec<&str> = design_records()
        .iter()
        .map(|record| record.source.name.as_str())
        .collect();
    assert!(!names.is_empty());
    let missing = missing_sections(names, &markdown);
    assert!(
        missing.is_empty(),
        "design-data.json cites sources with no section in THIRD_PARTY_NOTICES.md: {missing:?}"
    );
}

#[test]
fn the_notice_checker_catches_a_missing_commented_or_misleveled_section() {
    let real = notices_markdown();
    // Not vacuous: a non-seed source is reported until its section exists.
    assert_eq!(missing_sections(["Some Dataset"], &real), ["Some Dataset"]);
    // The seed needs none; a real section satisfies the check.
    assert!(missing_sections([SEED_SOURCE], "").is_empty());
    assert!(missing_sections(["Some Dataset"], "# Notices\n\n## Some Dataset\n\nMIT\n").is_empty());
    // The placeholder for the next step is a comment and does not count.
    assert!(real.contains("## UI UX Pro Max"), "placeholder disappeared");
    assert_eq!(
        missing_sections(["UI UX Pro Max"], &real),
        ["UI UX Pro Max"]
    );
    // Wrong heading level, a mere substring and a prefix do not count.
    assert_eq!(missing_sections(["X"], "### X\n"), ["X"]);
    assert_eq!(missing_sections(["X"], "## X and more\n"), ["X"]);
    assert_eq!(
        missing_sections(["Some Dataset"], "## Some\n"),
        ["Some Dataset"]
    );
    // An unterminated comment hides everything after it.
    assert_eq!(missing_sections(["X"], "<!-- ## X\n"), ["X"]);
}
