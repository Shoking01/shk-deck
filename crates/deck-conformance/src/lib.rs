//! # deck-conformance
//!
//! One suite, every deck.
//!
//! The point of a seam is that a second deck drops into it. The point of *this*
//! crate is that "drops in" is provable rather than aspirational: one file of
//! cases, parameterized by [`Deck`], so the same rules run against the real
//! device and against a simulator.
//!
//! The cases are deterministic and need nobody at a keyboard. They assert what
//! the seam promised: geometry that agrees with itself, images that match the
//! declared spec, out-of-range keys refused, unsupported operations refused
//! instead of ignored, and nothing that panics.
//!
//! ```
//! use deck_conformance::{run, CaseResult, Outcome};
//!
//! fn report(deck: &mut dyn deck_api::Deck) -> usize {
//!     let results: Vec<CaseResult> = run(deck);
//!     results.into_iter().filter(|case| case.outcome == Outcome::Pass).count()
//! }
//! ```

#![deny(unsafe_code)]

mod cases;
mod report;

pub use report::{CaseResult, Outcome};

use deck_api::Deck;

/// Runs every case against `deck` and returns one result per case.
///
/// The order is fixed, so a diff between two decks reads as a diff in the same
/// list. A case that fails does not stop the run: the caller wants the whole
/// picture, not the first problem.
///
/// ```
/// use deck_conformance::{run, Outcome};
///
/// fn all_pass(deck: &mut dyn deck_api::Deck) -> bool {
///     run(deck).into_iter().all(|case| case.outcome == Outcome::Pass)
/// }
/// ```
pub fn run(deck: &mut dyn Deck) -> Vec<CaseResult> {
    cases::all(deck)
}
