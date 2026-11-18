//! Text helpers of the info bar: middle elision of a path part and the width
//! of the breadcrumb separator.
//!
//! A project or folder name keeps both ends when it is shortened, because the
//! tail ("-v2", "checkout") is as telling as the head. A file name keeps its
//! extension instead; that case stays with `elide_filename_to_width`.

/// The glyph between breadcrumb parts.
pub(super) const SEPARATOR: &str = "›";

/// Shorten `text` to fit `max_width`, keeping its head and its tail around a
/// `…`. Returns the text unchanged when it already fits, and an empty string
/// when not even the ellipsis fits.
pub(super) fn elide_middle(
    text: &str,
    max_width: f32,
    mut measure: impl FnMut(&str) -> f32,
) -> String {
    if text.is_empty() || max_width <= 0.0 {
        return String::new();
    }
    if measure(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let candidate = |kept: usize| -> String {
        let head = kept - kept / 2;
        let tail = kept / 2;
        let mut out: String = chars[..head].iter().collect();
        out.push('…');
        out.extend(chars[chars.len() - tail..].iter());
        out
    };
    if measure("…") > max_width {
        return String::new();
    }
    // The widest candidate that fits: `kept` letters survive, split between
    // the two ends (the head gets the odd one).
    let (mut low, mut high) = (0usize, chars.len().saturating_sub(1));
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        if measure(&candidate(mid)) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    candidate(low)
}
