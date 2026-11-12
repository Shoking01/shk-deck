//! The errors a deck and a transport report.
//!
//! Every failure a caller can act on has its own variant. An operation the
//! deck does not support is [`DeckError::Unsupported`], not a generic error
//! and never a success that changed nothing.

use thiserror::Error;

use crate::types::KeyId;

/// What went wrong moving bytes to or from a device.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The device answered with an I/O error.
    #[error("transport I/O failed: {0}")]
    Io(String),

    /// The device stopped answering mid-transfer.
    #[error("transport stalled: {0}")]
    Stalled(String),

    /// Nothing arrived before the timeout.
    #[error("transport timed out")]
    Timeout,
}

/// What went wrong doing something to a deck.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DeckError {
    /// The deck does not do this at all.
    ///
    /// This is the error that keeps a second deck honest: a deck without
    /// brightness says so, instead of accepting the call and doing nothing.
    #[error("{operation} is not supported by this deck")]
    Unsupported {
        /// The operation that was refused, named as it appears in the code.
        operation: &'static str,
    },

    /// The key does not exist on this deck.
    #[error("key index {key} is out of range (this deck has {key_count} keys)")]
    KeyOutOfRange {
        /// The key that was asked for.
        key: KeyId,
        /// How many keys the deck actually has.
        key_count: u16,
    },

    /// The image is bigger than the deck accepts.
    #[error("image is {bytes} bytes; this deck accepts at most {max_bytes}")]
    ImageTooLarge {
        /// The size of the image that was offered.
        bytes: usize,
        /// The limit the deck states.
        max_bytes: usize,
    },

    /// The image is not the size the deck expects.
    #[error("image is {width}x{height}; this deck takes {expected_width}x{expected_height}")]
    ImageWrongSize {
        /// The width of the image that was offered.
        width: u16,
        /// The height of the image that was offered.
        height: u16,
        /// The width the deck expects.
        expected_width: u16,
        /// The height the deck expects.
        expected_height: u16,
    },

    /// There is no device to talk to.
    #[error("no device found")]
    DeviceNotFound,

    /// The bytes themselves failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
}
