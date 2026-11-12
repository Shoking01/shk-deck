//! # deck-api
//!
//! The seam between **what a deck can do** and **how its bytes travel**.
//!
//! Two traits carry the whole contract:
//!
//! - [`Deck`] — the semantics: how many keys the deck has, how they are laid
//!   out, what an image looks like on it, what it can do, and what it reports
//!   when a key is pressed.
//! - [`Transport`] — the bytes: one report out, two kinds of read in.
//!
//! A deck that cannot do something says so with a typed error
//! ([`DeckError::Unsupported`]). It never pretends to have done it: a silent
//! no-op is how a user ends up with a key that does nothing and no way to know
//! why.
//!
//! ## Why two traits
//!
//! A second deck differs in key count, geometry, image size, capabilities and
//! input decoding *before* it differs in transport. Cutting the seam at the
//! transport alone would leave every one of those differences inside the
//! driver, where they cannot be swapped.
//!
//! ## Example
//!
//! ```
//! use deck_api::{Deck, DeckError, KeyId};
//!
//! fn paint_first_key(deck: &mut dyn Deck, jpeg: &[u8]) -> Result<(), DeckError> {
//!     let first = KeyId::new(0);
//!     let geometry = deck.geometry();
//!     if !geometry.contains(first) {
//!         return Err(DeckError::KeyOutOfRange {
//!             key: first,
//!             key_count: geometry.key_count(),
//!         });
//!     }
//!     deck.set_key_image(first, jpeg)
//! }
//! ```
//!
//! This crate is pure: it does no I/O, reads no clock, and knows nothing about
//! any particular device.

#![deny(unsafe_code)]

mod deck;
mod error;
mod transport;
mod types;

pub use deck::{Deck, DeckHandle};
pub use error::{DeckError, TransportError};
pub use transport::Transport;
pub use types::{ButtonState, Capabilities, Geometry, ImageFormat, ImageSpec, KeyEvent, KeyId};
