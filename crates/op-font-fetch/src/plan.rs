//! Which files of a family to download: exactly the faces the document uses,
//! intersected with what the family offers.

use std::collections::BTreeSet;

use crate::metadata::{MetaFont, Metadata};
use crate::urls::is_safe_font_file_name;

/// Hard stop on files per family: a document can ask for at most 9 weights x
/// 2 styles, and a static family that offers more is not worth the disk.
pub const MAX_FILES_PER_FAMILY: usize = 18;

/// One face the document uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceRequest {
    pub weight: u16,
    pub italic: bool,
}

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub file_name: String,
    pub italic: bool,
    /// The face this file was chosen for (informational; a variable file
    /// serves every weight).
    pub weight: u16,
}

fn is_italic(font: &MetaFont) -> bool {
    font.style.eq_ignore_ascii_case("italic")
}

fn is_variable_file(name: &str) -> bool {
    name.contains('[')
}

/// Plan the file list. `None` when the family offers no usable `.ttf` at all.
///
/// - Variable family (a `wght` axis and bracketed file names): one file per
///   requested style, never the static weights as well (a variable file and a
///   static Regular would replace each other in the font store).
/// - Static family: for each requested face the file of that style with the
///   nearest weight (ties go to the lighter one).
/// - A request for a style the family lacks (italic of an upright-only
///   family) is dropped; when nothing remains, the family's regular face is
///   fetched so the missing-font row can still resolve.
pub fn plan_files(meta: &Metadata, wanted: &BTreeSet<FaceRequest>) -> Option<Vec<PlannedFile>> {
    let usable: Vec<&MetaFont> = meta
        .fonts
        .iter()
        .filter(|font| is_safe_font_file_name(&font.filename))
        .collect();
    if usable.is_empty() {
        return None;
    }
    let default_request = [FaceRequest {
        weight: 400,
        italic: false,
    }];
    let requests: Vec<FaceRequest> = if wanted.is_empty() {
        default_request.to_vec()
    } else {
        wanted.iter().copied().collect()
    };
    let variable =
        meta.weight_axis().is_some() && usable.iter().any(|font| is_variable_file(&font.filename));

    let mut files = choose_files(&usable, &requests, variable);
    if files.is_empty() {
        files = choose_files(&usable, &default_request, variable);
    }
    if files.is_empty() {
        // Not even an upright face of the right kind: take the first file.
        let first = usable[0];
        files.push(PlannedFile {
            file_name: first.filename.clone(),
            italic: is_italic(first),
            weight: first.weight,
        });
    }
    (files.len() <= MAX_FILES_PER_FAMILY).then_some(files)
}

fn choose_files(
    usable: &[&MetaFont],
    requests: &[FaceRequest],
    variable: bool,
) -> Vec<PlannedFile> {
    let mut files: Vec<PlannedFile> = Vec::new();
    for request in requests {
        let of_style = usable
            .iter()
            .copied()
            .filter(|font| is_italic(font) == request.italic)
            .filter(|font| !variable || is_variable_file(&font.filename));
        let chosen = if variable {
            of_style.into_iter().next()
        } else {
            of_style.min_by_key(|font| (font.weight.abs_diff(request.weight), font.weight))
        };
        if let Some(font) = chosen {
            if !files.iter().any(|f| f.file_name == font.filename) {
                files.push(PlannedFile {
                    file_name: font.filename.clone(),
                    italic: is_italic(font),
                    weight: request.weight,
                });
            }
        }
    }
    files
}
