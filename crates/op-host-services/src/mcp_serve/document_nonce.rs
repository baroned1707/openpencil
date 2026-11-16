//! The per-launch suffix of a live document id.
//!
//! Document ids are small counters that restart at 1 on every launch, so an
//! external agent that kept `d3` from an earlier session would silently edit
//! whatever document is `d3` now. The live server therefore names a document
//! `d<N>.<nonce>`, where the nonce is six lowercase hex characters chosen once
//! per launch: an id from a previous launch can never match.
//!
//! The headless single-document modes have no previous session to confuse
//! with, so they keep reporting and accepting the bare `d1`.

use std::fmt;
use std::sync::OnceLock;

/// Number of hex characters in a nonce.
const NONCE_LEN: usize = 6;

/// Six lowercase hex characters. `Copy` so it rides in routes and errors.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct LaunchNonce([u8; NONCE_LEN]);

impl LaunchNonce {
    /// The nonce spelled by `text`, or `None` unless it is exactly six
    /// lowercase hex characters (so one nonce has exactly one spelling).
    pub fn parse(text: &str) -> Option<Self> {
        let bytes: [u8; NONCE_LEN] = text.as_bytes().try_into().ok()?;
        bytes
            .iter()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            .then_some(Self(bytes))
    }

    /// A fresh random nonce, or `None` when the OS has no randomness.
    fn random() -> Option<Self> {
        let mut random = [0u8; NONCE_LEN / 2];
        getrandom::fill(&mut random).ok()?;
        let hex: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        Self::parse(&hex)
    }

    pub fn as_str(&self) -> &str {
        // Construction guarantees ASCII hex, so this never falls back.
        std::str::from_utf8(&self.0).unwrap_or("000000")
    }
}

impl fmt::Display for LaunchNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for LaunchNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LaunchNonce({})", self.as_str())
    }
}

/// This process's nonce, chosen on first use and stable for the rest of the
/// launch. A live server that is stopped and restarted on another port keeps
/// the ids an agent already holds.
///
/// Tests that need a different launch hand a nonce to the server constructor
/// instead of touching this.
pub fn launch_nonce() -> LaunchNonce {
    static NONCE: OnceLock<LaunchNonce> = OnceLock::new();
    *NONCE.get_or_init(|| {
        LaunchNonce::random().unwrap_or_else(|| {
            // No OS randomness: still never a fixed value across launches.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.subsec_nanos() ^ std::process::id())
                .unwrap_or(0);
            LaunchNonce::parse(&format!("{:06x}", nanos & 0x00ff_ffff))
                .unwrap_or(LaunchNonce(*b"000000"))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonce_is_exactly_six_lowercase_hex_characters() {
        assert!(LaunchNonce::parse("a1b2c3").is_some());
        for bad in ["", "a1b2c", "a1b2c3d", "A1B2C3", "a1b2cg", "a1b2c\u{e9}"] {
            assert!(LaunchNonce::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_launch_nonce_is_valid_and_stable_within_a_launch() {
        let first = launch_nonce();
        assert_eq!(LaunchNonce::parse(first.as_str()), Some(first));
        assert_eq!(launch_nonce(), first);
    }
}
