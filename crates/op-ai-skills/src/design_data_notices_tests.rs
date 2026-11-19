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

// ---------------------------------------------------------------------------
// Adapted prompt skills (design-craft, design-brief, design-wcag-quickcheck)
// ---------------------------------------------------------------------------

const ANTHROPIC_SKILLS: &str = "e4bb5d960d87126ff3b3a8af1a104b7db67560ba";
const IMPECCABLE: &str = "d41d9860b0094dcb68030657da63461212835aaa";
const DESIGNER_SKILLS: &str = "9a6930cf84a822eb458624bd11c61aac5bbdf224";
const KNOWLEDGE_WORK_PLUGINS: &str = "e91a134584eb798bac7fb0379358006a2b63b965";

const MODIFIED_STATEMENT: &str =
    "Adapted and condensed by OpenPencil for a node-based editor; modified.";

const ANTHROPIC_SECTION: &str =
    "Anthropic skills (frontend-design) and knowledge-work-plugins (write-spec) (Apache-2.0)";
const IMPECCABLE_SECTION: &str = "Impeccable (Apache-2.0)";
const OWL_SECTION: &str = "Owl-Listener designer-skills (MIT)";
const APACHE_SECTION: &str = "Apache License 2.0 (full text)";

const MENGTO_SKILLS: &str = "1a32b45a57f435f1564544e7267a71fc3b180a7a";
const TRYOPENDATA_SKILLS: &str = "f97583ecb7eeb8a0c2a193fe9b80e55806d29823";
const JEZWEB_SKILLS: &str = "1260fa7439b36b18dbf16648ccc7fb44783f6c23";
const LOGO_DESIGN_SKILL: &str = "e9eaddc07895575253d6663320354a708f53e955";
const DRAW_BETTER_SVG: &str = "5e65ee36fb4ad431267defcac268b020d096d7f4";
const VECTOR_SECTION: &str = "Vector drawing skills (MIT)";

/// Skill -> the third-party files it was adapted from: (path, commit).
const ADAPTED_SKILLS: [(&str, &[(&str, &str)]); 3] = [
    (
        "design-craft",
        &[
            ("skills/frontend-design/SKILL.md", ANTHROPIC_SKILLS),
            ("skill/reference/craft-floor.md", IMPECCABLE),
            ("skill/reference/typeset.md", IMPECCABLE),
            ("skill/reference/colorize.md", IMPECCABLE),
            ("skill/reference/layout.md", IMPECCABLE),
            ("skill/reference/critique-rubric.md", IMPECCABLE),
        ],
    ),
    (
        "design-brief",
        &[
            ("ux-strategy/skills/design-brief/SKILL.md", DESIGNER_SKILLS),
            (
                "ux-strategy/skills/information-architecture/SKILL.md",
                DESIGNER_SKILLS,
            ),
            (
                "prototyping-testing/skills/user-flow-diagram/SKILL.md",
                DESIGNER_SKILLS,
            ),
            (
                "product-management/skills/write-spec/SKILL.md",
                KNOWLEDGE_WORK_PLUGINS,
            ),
        ],
    ),
    (
        "design-vector-drawing",
        &[
            (
                "agent-skills/illustration/illustration-flat/references/craft.md",
                MENGTO_SKILLS,
            ),
            (
                "plugins/opendesign/skills/svg-design/references/icon-design.md",
                TRYOPENDATA_SKILLS,
            ),
            (
                "plugins/design-assets/skills/icon-set-generator/SKILL.md",
                JEZWEB_SKILLS,
            ),
            (
                "skills/logo-design/references/svg-construction.md",
                LOGO_DESIGN_SKILL,
            ),
            (
                "plugins/draw-better-svg/skills/draw-better-svg/references/review-and-repair.md",
                DRAW_BETTER_SVG,
            ),
        ],
    ),
];

/// Impeccable's NOTICE.md at the pinned commit (sha256 c60a093c...), verbatim.
const IMPECCABLE_NOTICE: &str = "# Third-Party Notices\n\
\n\
This project includes content derived from third-party work, used under the terms of its original license.\n\
\n\
## Platform Design Skills\n\
\n\
The `skill/reference/ios.md` and `skill/reference/android.md` platform reference files are distilled from ehmo's `platform-design-skills` (Apple Human Interface Guidelines and Material Design 3 rules), rewritten in Impeccable's voice.\n\
\n\
**Original work:** https://github.com/ehmo/platform-design-skills\n\
**Original license:** MIT\n\
**Author:** ehmo";

/// Body of the level-3 subsection `### heading`, up to the next heading.
fn subsection_body<'a>(markdown: &'a str, heading: &str) -> &'a str {
    let marker = format!("\n### {heading}\n");
    let start = markdown
        .find(&marker)
        .unwrap_or_else(|| panic!("no subsection {heading:?}"))
        + marker.len();
    let rest = &markdown[start..];
    rest.find("\n#").map_or(rest, |end| &rest[..end])
}

#[test]
fn adapted_skills_have_notices_with_sources() {
    let markdown = notices_markdown();
    let list = section_body(&markdown, "Adapted prompt skills");
    for (skill, sources) in ADAPTED_SKILLS {
        assert!(
            crate::get_skill_by_name(skill).is_some(),
            "{skill} is not a registered skill"
        );
        let body = subsection_body(list, skill);
        assert!(
            body.contains(MODIFIED_STATEMENT),
            "{skill}: the notice must say the work was modified"
        );
        for (path, commit) in sources {
            assert!(
                body.contains(path) && body.contains(&format!("`{commit}`")),
                "{skill}: source {path} at {commit} is not listed"
            );
        }
    }
    // The WCAG skill copies nothing and says so, with the specification link.
    let wcag = subsection_body(list, "design-wcag-quickcheck");
    assert!(wcag.contains("independent summary"));
    assert!(wcag.contains("https://www.w3.org/TR/WCAG22/"));
    // Each source project has its section, with the right copyright line.
    let headings = section_headings(&markdown);
    for section in [
        ANTHROPIC_SECTION,
        IMPECCABLE_SECTION,
        OWL_SECTION,
        APACHE_SECTION,
    ] {
        assert!(headings.contains(section), "missing section {section:?}");
    }
    assert!(section_body(&markdown, ANTHROPIC_SECTION).contains(ANTHROPIC_SKILLS));
    assert!(section_body(&markdown, ANTHROPIC_SECTION).contains(KNOWLEDGE_WORK_PLUGINS));
    assert!(section_body(&markdown, ANTHROPIC_SECTION).contains(MODIFIED_STATEMENT));
    assert!(section_body(&markdown, IMPECCABLE_SECTION).contains(IMPECCABLE));
    assert!(section_body(&markdown, IMPECCABLE_SECTION).contains(MODIFIED_STATEMENT));
    let owl = section_body(&markdown, OWL_SECTION);
    assert!(owl.contains(DESIGNER_SKILLS));
    assert!(owl.contains("Copyright (c) 2026 MC Dean"));
    assert!(owl.contains("Permission is hereby granted, free of charge"));
    assert!(owl.contains(MODIFIED_STATEMENT));
}

#[test]
fn apache_text_and_impeccable_notice_are_present() {
    let markdown = notices_markdown();
    // The Apache text appears once, whole: its title, every numbered
    // section heading and the closing line.
    let apache = section_body(&markdown, APACHE_SECTION);
    assert!(apache.contains("Apache License\n                           Version 2.0, January 2004"));
    assert!(apache.contains("TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"));
    for clause in [
        "1. Definitions.",
        "2. Grant of Copyright License.",
        "3. Grant of Patent License.",
        "4. Redistribution.",
        "5. Submission of Contributions.",
        "6. Trademarks.",
        "7. Disclaimer of Warranty.",
        "8. Limitation of Liability.",
        "9. Accepting Warranty or Additional Liability.",
    ] {
        assert!(apache.contains(clause), "Apache text lacks {clause:?}");
    }
    assert_eq!(markdown.matches("END OF TERMS AND CONDITIONS").count(), 1);
    assert_eq!(markdown.matches("Grant of Patent License").count(), 1);

    // Impeccable: copyright line, the NOTICE text line for line (indented),
    // and the exclusion statement.
    let impeccable = section_body(&markdown, IMPECCABLE_SECTION);
    assert!(impeccable.contains("Copyright 2025 Paul Bakaus"));
    for line in IMPECCABLE_NOTICE.lines().filter(|line| !line.is_empty()) {
        assert!(
            impeccable.contains(&format!("    {line}")),
            "Impeccable NOTICE line missing: {line:?}"
        );
    }
    for excluded in ["ios.md", "android.md", "*.native.md"] {
        assert!(
            impeccable.contains(excluded),
            "exclusion of {excluded} not stated"
        );
    }
}

#[test]
fn notice_check_fails_without_a_section() {
    let markdown = notices_markdown();
    let required = [
        ANTHROPIC_SECTION,
        IMPECCABLE_SECTION,
        OWL_SECTION,
        APACHE_SECTION,
    ];
    assert!(missing_sections(required, &markdown).is_empty());
    // Negative proof on real files: cut one section out of a temp copy and the
    // check must name exactly that section.
    for (index, section) in required.iter().enumerate() {
        let start = markdown
            .find(&format!("\n## {section}\n"))
            .expect("section present")
            + 1;
        let end = markdown[start..]
            .find("\n## ")
            .map_or(markdown.len(), |next| start + next + 1);
        let directory = std::env::temp_dir().join(format!(
            "op-ai-skills-adapted-notices-{}-{index}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("temp dir");
        let copy = directory.join("THIRD_PARTY_NOTICES.md");
        std::fs::write(&copy, format!("{}{}", &markdown[..start], &markdown[end..]))
            .expect("write copy");
        let reduced = std::fs::read_to_string(&copy).expect("read copy");
        std::fs::remove_dir_all(&directory).expect("clean up");
        assert_eq!(missing_sections(required, &reduced), [*section]);
    }
}

#[test]
fn vector_drawing_section_names_every_source_with_its_copyright_and_one_mit_text() {
    let markdown = notices_markdown();
    assert!(section_headings(&markdown).contains(VECTOR_SECTION));
    let body = section_body(&markdown, VECTOR_SECTION);
    for (repository, commit, copyright) in [
        (
            "https://github.com/MengTo/Skills",
            MENGTO_SKILLS,
            "Copyright (c) 2026 Meng To",
        ),
        (
            "https://github.com/tryopendata/skills",
            TRYOPENDATA_SKILLS,
            "Copyright (c) 2026 OpenData",
        ),
        (
            "https://github.com/jezweb/claude-skills",
            JEZWEB_SKILLS,
            "Copyright (c) 2025 Jeremy Dawes (Jezweb)",
        ),
        (
            "https://github.com/kaankiziltug/logo-design-skill",
            LOGO_DESIGN_SKILL,
            "Copyright (c) 2026 kaankiziltug",
        ),
        (
            "https://github.com/arlinamid/draw-better-svg",
            DRAW_BETTER_SVG,
            "Copyright (c) 2026 Rózsavölgyi János",
        ),
    ] {
        assert!(body.contains(repository), "{repository} is not named");
        assert!(
            body.contains(&format!("`{commit}`")),
            "{repository}: no pin"
        );
        assert!(body.contains(copyright), "{repository}: no {copyright:?}");
    }
    assert!(body.contains(MODIFIED_STATEMENT));
    // One licence text, whole, shared by the five sources.
    assert_eq!(body.matches("Permission is hereby granted").count(), 1);
    assert!(body.contains("THE SOFTWARE IS PROVIDED \"AS IS\""));
    assert!(body.contains("SOFTWARE.\n```"));
    // The trademarked sample logos are named as NOT used.
    assert!(body.contains("trademarked") && body.contains("not used"));
}
