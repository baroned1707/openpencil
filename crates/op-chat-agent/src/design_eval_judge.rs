//! The seam for a future model-based visual judge (an interface only: there is
//! no implementation, no network code and nothing calls it).
//!
//! A judge compares a baseline screenshot with a candidate screenshot of the
//! same eval case and says which looks better. It is meant as an advisory
//! column next to the metrics, never as a gate on its own.
//!
//! Calibration rule (owner decision): a judge may gate CI only after the owner
//! has labelled about 20 baseline / candidate pairs and the judge agrees with
//! those labels on at least 80% of the pairs the owner did not call a tie.
//! The rule applies again whenever the judge model changes. Until then every
//! verdict is advisory.
//!
//! Position bias: a caller asks twice, once with the images swapped, and counts
//! a win only when both orders name the same winner; any disagreement is a
//! [`Verdict::Tie`].

/// Which image won a pairwise comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Baseline,
    Candidate,
    Tie,
}

/// A visual judge over two PNG screenshots of one eval case.
pub trait DesignJudge {
    /// Compare `baseline_png` with `candidate_png` for the eval case `case_id`.
    fn pairwise(&self, case_id: &str, baseline_png: &[u8], candidate_png: &[u8]) -> Verdict;
}
