//! The credit lines of a document: who made each stock image and under which
//! licence, ready to paste next to the exported design.
//!
//! The data is the `imageCredit` stored on the nodes that own a picture; this
//! module only reads it. [`collect_image_credits`] is the document walk and
//! [`format_credits_text`] the text, both pure so they are table-tested
//! without an editor or a network.

use std::collections::HashSet;

use jian_ops_schema::node::PenNode;
use jian_ops_schema::PenDocument;
use op_editor_core::{node_image_credit, node_image_source, PenNodeExt as _};

use crate::license_policy::{license_policy_with, LicensePolicy};
use crate::ImageCredit;

/// One image credit of a document and the node it was found on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditLine {
    /// The name of the first node that carries this credit (its id when the
    /// node has no name). The credit itself holds no title.
    pub title: String,
    pub credit: ImageCredit,
}

/// Every distinct image credit of `doc`, in reading order: pages in order,
/// each page depth first. Identical credits (the same photo used on several
/// nodes) appear once, under the first node that carries them; credits that
/// differ in any field (another photo by the same creator) stay separate.
/// A credit counts only on a node that still paints a picture.
pub fn collect_image_credits(doc: &PenDocument) -> Vec<CreditLine> {
    let mut lines = Vec::new();
    let mut seen = HashSet::new();
    match doc.pages.as_deref() {
        Some(pages) if !pages.is_empty() => {
            for page in pages {
                collect_from(&page.children, &mut seen, &mut lines);
            }
        }
        _ => collect_from(&doc.children, &mut seen, &mut lines),
    }
    lines
}

type CreditKey = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn credit_key(credit: &ImageCredit) -> CreditKey {
    (
        credit.provider.clone(),
        credit.creator.clone(),
        credit.license.clone(),
        credit.license_url.clone(),
        credit.source_page_url.clone(),
    )
}

fn collect_from(nodes: &[PenNode], seen: &mut HashSet<CreditKey>, lines: &mut Vec<CreditLine>) {
    for node in nodes {
        if let Some(credit) = node_image_credit(node) {
            let paints_a_picture = node_image_source(node)
                .iter()
                .any(|source| !source.trim().is_empty());
            if paints_a_picture && seen.insert(credit_key(credit)) {
                lines.push(CreditLine {
                    title: node
                        .base()
                        .name
                        .clone()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| node.id_str().to_string()),
                    credit: credit.clone(),
                });
            }
        }
        if let Some(children) = node.children() {
            collect_from(children, seen, lines);
        }
    }
}

/// One line per credit:
/// `"<title>" by <creator> — <licence> — <licence url> — <source url>`,
/// leaving out every field the source did not report. A public-domain / CC0
/// line ends with `no credit required`, a CC BY line with `credit required`;
/// any other licence carries no marker rather than a guess. Lines are joined
/// by `\n` (no trailing newline); no credits give an empty string.
pub fn format_credits_text(lines: &[CreditLine]) -> String {
    lines.iter().map(format_line).collect::<Vec<_>>().join("\n")
}

fn format_line(line: &CreditLine) -> String {
    let credit = &line.credit;
    let mut text = format!("\"{}\"", line.title);
    if let Some(creator) = non_empty(&credit.creator) {
        text.push_str(&format!(" by {creator}"));
    }
    for field in [
        &credit.license,
        &credit.license_url,
        &credit.source_page_url,
    ] {
        if let Some(value) = non_empty(field) {
            text.push_str(&format!(" — {value}"));
        }
    }
    if let Some(marker) = credit_marker(credit) {
        text.push_str(&format!(" — {marker}"));
    }
    text
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Share-alike counts as "credit required" here: a stored credit is a licence
/// that was accepted, and every CC licence except CC0 / PDM asks for credit.
fn credit_marker(credit: &ImageCredit) -> Option<&'static str> {
    match license_policy_with(
        credit.license.as_deref().unwrap_or(""),
        credit.license_url.as_deref().unwrap_or(""),
        true,
    ) {
        LicensePolicy::Allow => Some("no credit required"),
        LicensePolicy::AllowWithCredit => Some("credit required"),
        LicensePolicy::Refuse(_) => None,
    }
}

#[cfg(test)]
#[path = "credits_tests.rs"]
mod tests;
