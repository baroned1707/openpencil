//! A search hit as the writers consume it: the url to land and the credit
//! that goes onto the node with it.

use crate::ImageCredit;

/// One chosen image. `credit` is `None` for a result no provider vouches for
/// (a hand-built url, a test fixture); landing it then clears any credit the
/// slot held.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FoundImage {
    pub url: String,
    pub credit: Option<ImageCredit>,
}

impl From<String> for FoundImage {
    fn from(url: String) -> Self {
        Self { url, credit: None }
    }
}

impl From<&str> for FoundImage {
    fn from(url: &str) -> Self {
        Self::from(url.to_string())
    }
}
