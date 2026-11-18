use super::*;
use serde_json::json;

fn credit(creator: &str, license: &str, source: &str) -> serde_json::Value {
    json!({
        "provider": "openverse", "creator": creator, "license": license,
        "licenseUrl": format!("https://creativecommons.org/licenses/{}/4.0/", license.to_lowercase()),
        "sourcePageUrl": source
    })
}

fn document(value: serde_json::Value) -> PenDocument {
    serde_json::from_value(value).expect("fixture document")
}

#[test]
fn collect_image_credits_dedupes_and_orders() {
    let jane_a = credit("Jane", "by", "https://example.org/a");
    let jane_b = credit("Jane", "by", "https://example.org/b");
    let doc = document(json!({
        "version": "1.0.0",
        "pages": [
            {"id": "p1", "name": "One", "children": [
                {"type": "frame", "id": "card", "name": "Card", "imageCredit": jane_a,
                 "fill": [{"type": "image", "url": "data:image/png;base64,AA=="}],
                 "children": [
                    {"type": "image", "id": "inner", "name": "Inner", "src": "https://x/1.png",
                     "imageCredit": credit("Zed", "by", "https://example.org/z")}
                 ]},
                // Same photo again, another node: one line, first node's name.
                {"type": "image", "id": "again", "name": "Again", "src": "https://x/2.png",
                 "imageCredit": jane_a}
            ]},
            {"id": "p2", "name": "Two", "children": [
                // Same creator, different photo: its own line.
                {"type": "image", "id": "other", "name": "", "src": "https://x/3.png",
                 "imageCredit": jane_b},
                // A credit on a node that paints no picture is stale: ignored.
                {"type": "image", "id": "empty", "name": "Empty", "src": "",
                 "imageCredit": credit("Ghost", "by", "https://example.org/g")}
            ]}
        ]
    }));

    let lines = collect_image_credits(&doc);
    let summary: Vec<(&str, Option<&str>, Option<&str>)> = lines
        .iter()
        .map(|line| {
            (
                line.title.as_str(),
                line.credit.creator.as_deref(),
                line.credit.source_page_url.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Card", Some("Jane"), Some("https://example.org/a")),
            ("Inner", Some("Zed"), Some("https://example.org/z")),
            // A nameless node falls back to its id.
            ("other", Some("Jane"), Some("https://example.org/b")),
        ]
    );
}

#[test]
fn a_document_without_credits_has_none() {
    let doc = document(json!({
        "version": "1.0.0",
        "children": [{"type": "image", "id": "a", "src": "https://x/1.png"}]
    }));
    assert!(collect_image_credits(&doc).is_empty());
    assert_eq!(format_credits_text(&[]), "");
}

fn line(
    title: &str,
    creator: Option<&str>,
    license: Option<&str>,
    url: Option<&str>,
) -> CreditLine {
    CreditLine {
        title: title.into(),
        credit: ImageCredit {
            provider: "openverse".into(),
            creator: creator.map(Into::into),
            license: license.map(Into::into),
            license_url: url.map(Into::into),
            source_page_url: Some("https://example.org/work".into()),
        },
    }
}

#[test]
fn credits_text_marks_credit_required_vs_not() {
    let text = format_credits_text(&[
        line(
            "Hero",
            Some("Jane Doe"),
            Some("CC BY 4.0"),
            Some("https://creativecommons.org/licenses/by/4.0/"),
        ),
        line("Sky", Some("Bob"), Some("CC0 1.0"), None),
        line("Map", None, Some("Public domain"), None),
        line("Wave", Some("Ann"), Some("CC BY-NC 4.0"), None),
        line("Bare", None, None, None),
    ]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        vec![
            "\"Hero\" by Jane Doe — CC BY 4.0 — https://creativecommons.org/licenses/by/4.0/ — https://example.org/work — credit required",
            "\"Sky\" by Bob — CC0 1.0 — https://example.org/work — no credit required",
            "\"Map\" — Public domain — https://example.org/work — no credit required",
            // A licence the policy does not vouch for carries no marker.
            "\"Wave\" by Ann — CC BY-NC 4.0 — https://example.org/work",
            "\"Bare\" — https://example.org/work",
        ]
    );
    assert!(!text.ends_with('\n'));
}
