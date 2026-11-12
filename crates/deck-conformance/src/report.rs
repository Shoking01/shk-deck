//! What one case found.

use std::fmt;

/// How a case ended.
///
/// There are three outcomes, not two. A case that could not be judged is not a
/// case that passed: a deck that cannot be exercised has to say so, or the
/// suite reports a clean bill of health it never earned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The deck did what the case required.
    Pass,
    /// The deck did not, and here is why.
    Fail,
    /// The case could not reach a verdict against this deck.
    ///
    /// This is not a pass. A deck whose `image_spec` cannot be read, or whose
    /// transport is absent, is a deck the suite could not judge.
    NotApplicable,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::NotApplicable => "not applicable",
        };
        f.write_str(name)
    }
}

/// The result of one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResult {
    /// The case's stable name, snake_case, as it appears in a report.
    pub name: &'static str,
    /// How it ended.
    pub outcome: Outcome,
    /// Why, in one line. Empty on a pass.
    pub detail: String,
}

impl CaseResult {
    /// Builds a result.
    ///
    /// ```
    /// use deck_conformance::{CaseResult, Outcome};
    ///
    /// let passed = CaseResult::pass("geometry_is_self_consistent");
    /// assert_eq!(passed.outcome, Outcome::Pass);
    /// assert_eq!(passed.name, "geometry_is_self_consistent");
    /// ```
    pub fn pass(name: &'static str) -> Self {
        Self {
            name,
            outcome: Outcome::Pass,
            detail: String::new(),
        }
    }

    /// Builds a failing result with the reason.
    ///
    /// ```
    /// use deck_conformance::{CaseResult, Outcome};
    ///
    /// let failed = CaseResult::fail("key_out_of_range_is_refused", "key 6 was accepted");
    /// assert_eq!(failed.outcome, Outcome::Fail);
    /// assert_eq!(failed.detail, "key 6 was accepted");
    /// ```
    pub fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            outcome: Outcome::Fail,
            detail: detail.into(),
        }
    }

    /// Builds a result the case could not judge.
    ///
    /// ```
    /// use deck_conformance::{CaseResult, Outcome};
    ///
    /// let skipped = CaseResult::not_applicable("brightness_is_settable", "deck has no brightness");
    /// assert_eq!(skipped.outcome, Outcome::NotApplicable);
    /// ```
    pub fn not_applicable(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            outcome: Outcome::NotApplicable,
            detail: detail.into(),
        }
    }

    /// Whether the case passed.
    ///
    /// ```
    /// use deck_conformance::CaseResult;
    ///
    /// assert!(CaseResult::pass("x").passed());
    /// assert!(!CaseResult::fail("x", "no").passed());
    /// ```
    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Pass
    }
}
