//! Comparing secrets without leaking how much of a guess was right.
//!
//! `==` on strings stops at the first differing byte, so how long a wrong
//! guess takes to refuse tells an attacker how many leading bytes were right.
//! Every comparison of the live endpoint's credentials (the opt-in bearer
//! token, the `openpencil/shutdown` token) goes through [`secrets_equal`].

/// Whether `left` and `right` are the same bytes. Visits every byte of the
/// longer input whatever the content; only the lengths are not hidden (they
/// are public: our tokens have a fixed length).
pub fn secrets_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(a ^ b);
    }
    // `black_box` keeps the optimiser from turning the accumulated difference
    // back into an early exit.
    std::hint::black_box(difference) == 0
}

#[cfg(test)]
mod tests {
    use super::secrets_equal;

    #[test]
    fn equal_secrets_match() {
        assert!(secrets_equal(b"", b""));
        assert!(secrets_equal(b"abc123", b"abc123"));
    }

    #[test]
    fn any_difference_in_content_or_length_is_a_mismatch() {
        assert!(!secrets_equal(b"abc123", b"abc124"));
        assert!(!secrets_equal(b"abc123", b"bbc123"));
        assert!(!secrets_equal(b"abc123", b"abc12"));
        assert!(!secrets_equal(b"abc12", b"abc123"));
        // A shorter secret is not "equal" to a longer one padded with zeros.
        assert!(!secrets_equal(b"abc", b"abc\0\0"));
        assert!(!secrets_equal(b"", b"\0"));
    }
}
