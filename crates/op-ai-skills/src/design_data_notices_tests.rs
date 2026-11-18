//! Attribution gate: every data source other than the project's own seed must
//! have its licence section in `packaging/shared/THIRD_PARTY_NOTICES.md`, so
//! the importer of a new dataset cannot ship without attribution.

use std::collections::BTreeSet;
use std::path::Path;

use super::design_records;

const SEED_SOURCE: &str = "OpenPencil seed";
const IMPORT_SOURCE: &str = "UI UX Pro Max (MIT)";

/// The upstream licence, verbatim from the LICENSE file at the pinned commit
/// (`7436a6e0...`, sha256 738f69df...).
const UPSTREAM_MIT_TEXT: &str = "MIT License

Copyright (c) 2024 Next Level Builder

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the \"Software\"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.";

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
    // The imported dataset's section exists; a prefix of its name does not
    // satisfy the check, and a heading inside a comment never does.
    assert!(missing_sections([IMPORT_SOURCE], &real).is_empty());
    assert_eq!(
        missing_sections(["UI UX Pro Max"], &real),
        ["UI UX Pro Max"]
    );
    assert_eq!(
        missing_sections([IMPORT_SOURCE], "<!--\n## UI UX Pro Max (MIT)\n-->\n"),
        [IMPORT_SOURCE]
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

/// Body of the level-2 section called `heading`, up to the next level-2 heading.
fn section_body<'a>(markdown: &'a str, heading: &str) -> &'a str {
    let marker = format!("\n## {heading}\n");
    let start = markdown
        .find(&marker)
        .unwrap_or_else(|| panic!("no section {heading:?}"))
        + marker.len();
    let rest = &markdown[start..];
    rest.find("\n## ").map_or(rest, |end| &rest[..end])
}

#[test]
fn notices_have_the_uiuxpm_section_with_full_mit_text() {
    let markdown = notices_markdown();
    assert!(section_headings(&markdown).contains(IMPORT_SOURCE));
    let body = section_body(&markdown, IMPORT_SOURCE);
    // Attribution: where it came from, exactly which version, who owns it.
    assert!(body.contains("https://github.com/nextlevelbuilder/ui-ux-pro-max-skill"));
    assert!(body.contains("Copyright (c) 2024 Next Level Builder"));
    // The full licence text, byte for byte (so nothing was shortened).
    assert!(
        body.contains(UPSTREAM_MIT_TEXT),
        "the section must carry the complete upstream MIT licence text"
    );
    // The pinned commit in the notice is the commit every record cites.
    let imported: Vec<_> = design_records()
        .iter()
        .filter(|record| record.source.name == IMPORT_SOURCE)
        .collect();
    assert!(!imported.is_empty());
    for record in &imported {
        assert!(
            body.contains(&format!("`{}`", record.source.commit)),
            "{} cites commit {} which the notice does not name",
            record.id,
            record.source.commit
        );
        assert_eq!(record.source.license_spdx, "MIT");
    }
    // Modifications and omissions are stated.
    for statement in [
        "converted to JSON",
        "columns were dropped",
        "joined by product type",
        "filtered out",
        "Deliberately NOT included",
    ] {
        assert!(body.contains(statement), "notice lacks {statement:?}");
    }
}

#[test]
fn a_temp_copy_without_the_section_fails_the_check() {
    // Negative proof on a real file: remove the section from a copy and the
    // very check that passes on the shipped file must report the source.
    let markdown = notices_markdown();
    let start = markdown
        .find(&format!("## {IMPORT_SOURCE}"))
        .expect("section present");
    let directory = std::env::temp_dir().join(format!(
        "op-ai-skills-notices-{}-{}",
        std::process::id(),
        start
    ));
    std::fs::create_dir_all(&directory).expect("temp dir");
    let copy = directory.join("THIRD_PARTY_NOTICES.md");
    std::fs::write(&copy, &markdown[..start]).expect("write copy");
    let truncated = std::fs::read_to_string(&copy).expect("read copy");
    std::fs::remove_dir_all(&directory).expect("clean up");

    let names: Vec<&str> = design_records()
        .iter()
        .map(|record| record.source.name.as_str())
        .collect();
    assert!(names.contains(&IMPORT_SOURCE));
    assert!(missing_sections(names.iter().copied(), &markdown).is_empty());
    assert_eq!(missing_sections(names, &truncated), [IMPORT_SOURCE]);
    // And an unknown source never passes against the shipped file.
    assert_eq!(
        missing_sections(["Unknown Dataset (MIT)"], &markdown),
        ["Unknown Dataset (MIT)"]
    );
}
