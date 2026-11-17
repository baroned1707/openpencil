//! Every URL this crate can ever request is built here, from validated tokens.
//! The document's family string is never an input to any function in this
//! module.

use crate::catalog::is_valid_catalog_id;
use crate::license::License;

const RAW_BASE: &str = "https://raw.githubusercontent.com/google/fonts/main";
const MAX_FILE_NAME_CHARS: usize = 120;

/// google/fonts directory name for a catalog id: the id without dashes
/// (`roboto-slab` -> `robotoslab`). Observed convention, not a guarantee; a
/// wrong guess is a 404 and the family is reported as not downloadable.
pub fn directory_token(catalog_id: &str) -> Option<String> {
    if !is_valid_catalog_id(catalog_id) {
        return None;
    }
    let token: String = catalog_id.chars().filter(|c| *c != '-').collect();
    (!token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    .then_some(token)
}

/// A file name read from `METADATA.pb` (untrusted): a plain `.ttf` name with
/// only the characters Google's variable-font names use (`[`, `]`, `,`).
pub fn is_safe_font_file_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || name.chars().count() > MAX_FILE_NAME_CHARS || name.contains("..") {
        return false;
    }
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'[' | b']' | b','))
    {
        return false;
    }
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".ttf")
}

fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn directory_url(license: License, directory: &str) -> String {
    format!("{RAW_BASE}/{}/{directory}", license.directory())
}

pub fn metadata_url(license: License, directory: &str) -> String {
    format!("{}/METADATA.pb", directory_url(license, directory))
}

pub fn license_url(license: License, directory: &str) -> String {
    format!(
        "{}/{}",
        directory_url(license, directory),
        license.file_name()
    )
}

/// `file_name` must already satisfy [`is_safe_font_file_name`].
pub fn font_file_url(license: License, directory: &str, file_name: &str) -> String {
    format!(
        "{}/{}",
        directory_url(license, directory),
        encode_path_segment(file_name)
    )
}
