//! # deck-simulator
//!
//! Two fakes at the two seams, and neither of them is the device the seam was
//! written for. That is the whole point: a seam proved by a copy of the first
//! implementation proves nothing.
//!
//! - [`Ss550WireFake`] is a [`Transport`](deck_api::Transport). It speaks the
//!   real SS550 pack format — the packs are built by, and decoded with,
//!   `ss550-protocol` — records the commands it understood, and can be told to
//!   fail or stall on demand. A deck sits on it that never touches `hidapi`.
//! - [`SixKeyDeck`] is a [`Deck`](deck_api::Deck). Six keys in two rows of
//!   three, 72×72 images, two dials, no brightness and no sleep. It touches no
//!   transport at all.
//!
//! The pair is independent on purpose: the wire fake proves the bytes can be
//! swapped under a deck, and the six-key deck proves the deck can be swapped
//! without the bytes, the geometry, the image size or the capabilities coming
//! along. Neither depends on the other, and neither depends on a device.
//!
//! ## What this crate is not
//!
//! It is not a hardware safety gate. The verified-command registry is enforced
//! by the driver, before the bytes leave the host. These are test doubles, and
//! the wire fake deliberately accepts anything shaped like a pack. Do not wire
//! this crate into a path that talks to a real deck.
//!
//! ```
//! use deck_api::Deck;
//! use deck_conformance::Outcome;
//! use deck_simulator::SixKeyDeck;
//!
//! let mut deck = SixKeyDeck::new();
//! let failures = deck_conformance::run(&mut deck)
//!     .into_iter()
//!     .filter(|case| case.outcome == Outcome::Fail)
//!     .count();
//! assert_eq!(failures, 0);
//! ```

#![deny(unsafe_code)]

mod six_key_deck;
mod wire;
mod wire_fake;

pub use six_key_deck::SixKeyDeck;
pub use wire::Ss550Command;
pub use wire_fake::Ss550WireFake;
