//! # deck-conformance
//!
//! One suite, every deck.
//!
//! The point of a seam is that a second deck drops into it. The point of *this*
//! crate is that "drops in" is provable rather than aspirational: one file of
//! cases, parameterized by [`Deck`], so the same rules run against the real
//! device and against a simulator.
//!
//! What the cases check, all of it through the seam:
//!
//! - the image spec the deck declares is usable at all;
//! - a key past the end is refused, and the refusal names the key count the
//!   deck actually declares;
//! - an image larger than the declared limit is refused, and the refusal names
//!   the limit;
//! - a brightness past the range is refused instead of clamped;
//! - every operation the deck does not advertise — brightness, single-key
//!   clear, clear-all, sleep — is refused with [`DeckError::Unsupported`]
//!   instead of accepted and ignored;
//! - a dial that does not exist is refused, naming how many there are;
//! - an identity is never an empty success.
//!
//! What the cases deliberately do **not** do, and why:
//!
//! - **They never paint a key.** The suite runs against a device on a desk as
//!   well as against a simulator, so it only makes calls it expects to be
//!   refused. A successful paint belongs to the provider's own tests, which
//!   know how to build an image worth sending.
//! - **They cannot prove a provider validates before it writes.** A provider
//!   that wrote first and refused second would still pass. Safe hardware use
//!   rests on the provider checking cheaply before it touches the wire, which
//!   is the provider's rule to keep and its own tests' job to pin.
//! - **They never judge geometry arithmetic.** [`deck_api::Geometry`] derives
//!   its own rows and positions, so an inconsistent grid cannot be built; what
//!   a deck can get wrong is the key count it *reports*, and that is checked.
//!
//! ```no_run
//! use deck_conformance::{CaseResult, Outcome};
//!
//! fn report(deck: &mut dyn deck_api::Deck) -> usize {
//!     let results: Vec<CaseResult> = deck_conformance::run(deck);
//!     results.into_iter().filter(|case| case.outcome == Outcome::Pass).count()
//! }
//! ```
//!
//! The example is `no_run` on purpose: a conformance run talks to whatever deck
//! you handed it, and documentation should never open a device.

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
/// ```no_run
/// use deck_conformance::Outcome;
///
/// fn all_pass(deck: &mut dyn deck_api::Deck) -> bool {
///     deck_conformance::run(deck)
///         .into_iter()
///         .all(|case| case.outcome == Outcome::Pass)
/// }
/// ```
pub fn run(deck: &mut dyn Deck) -> Vec<CaseResult> {
    cases::all(deck)
}
