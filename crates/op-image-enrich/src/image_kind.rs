//! What a slot wants from a stock search: a photograph, or a vector / icon.
//!
//! The image node schema (`jian-ops-schema`, a vendored crate) has no field
//! for this and drops unknown fields on load, so the intent travels inside
//! the slot's existing search text as an opt-in prefix: `G(slot, "search",
//! "vector: shopping cart")` stores `imageSearchQuery = "vector: shopping
//! cart"`, and [`ImageKind::split_query`] turns it back into a kind plus the
//! keywords the providers see. A query without the prefix is a photo, so
//! every existing document and prompt behaves as before.

/// Opt-in marker at the start of a search text (case-insensitive).
pub const VECTOR_QUERY_PREFIX: &str = "vector:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageKind {
    #[default]
    Photo,
    Vector,
}

impl ImageKind {
    /// Stable lowercase name for cache keys and traces.
    pub fn as_str(self) -> &'static str {
        match self {
            ImageKind::Photo => "photo",
            ImageKind::Vector => "vector",
        }
    }

    /// Kind and provider keywords of a stored search text: `vector: home` is
    /// `(Vector, "home")`, anything else is `(Photo, text)` unchanged.
    pub fn split_query(text: &str) -> (ImageKind, &str) {
        let trimmed = text.trim_start();
        match trimmed.get(..VECTOR_QUERY_PREFIX.len()) {
            Some(head) if head.eq_ignore_ascii_case(VECTOR_QUERY_PREFIX) => (
                ImageKind::Vector,
                trimmed[VECTOR_QUERY_PREFIX.len()..].trim(),
            ),
            _ => (ImageKind::Photo, text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_without_prefix_is_a_photo() {
        assert_eq!(
            ImageKind::split_query("sunset over a lake"),
            (ImageKind::Photo, "sunset over a lake")
        );
        assert_eq!(ImageKind::default(), ImageKind::Photo);
        // A word that merely contains the marker is not the marker.
        assert_eq!(ImageKind::split_query("vectorial art").0, ImageKind::Photo);
    }

    #[test]
    fn prefix_opts_into_vector_and_is_stripped() {
        assert_eq!(
            ImageKind::split_query("vector: shopping cart"),
            (ImageKind::Vector, "shopping cart")
        );
        assert_eq!(
            ImageKind::split_query("  Vector:home"),
            (ImageKind::Vector, "home")
        );
        // Multi-byte text right at the prefix width must not panic.
        assert_eq!(ImageKind::split_query("ベクター").0, ImageKind::Photo);
    }
}
