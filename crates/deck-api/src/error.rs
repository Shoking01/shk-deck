//! The errors a deck and a transport report.
//!
//! Every failure a caller can act on has its own variant. An operation the
//! deck does not support is [`DeckError::Unsupported`], not a generic error
//! and never a success that changed nothing.

use std::fmt;

use thiserror::Error;

use crate::types::{KeyId, VALUE_MAX, VALUE_MIN};

/// An operation a deck was asked to do.
///
/// The seam names the refused operation with this type instead of a free-form
/// string: a typo in a string changes the message the user reads and nothing
/// notices. With this type, the compiler does.
///
/// ```
/// use deck_api::DeckOperation;
///
/// assert_eq!(DeckOperation::Sleep.to_string(), "sleep");
/// assert_eq!(DeckOperation::SetKeyImage.to_string(), "set_key_image");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeckOperation {
    /// Reading the deck's identity.
    Identity,
    /// Setting the display brightness.
    Brightness,
    /// Painting one key with an image.
    SetKeyImage,
    /// Clearing one key.
    ClearKey,
    /// Clearing every key.
    ClearAll,
    /// Putting the deck to sleep.
    Sleep,
    /// Moving one dial.
    SetDial,
    /// Reading the key events.
    PollEvents,
    /// Reopening the device.
    Reconnect,
}

impl fmt::Display for DeckOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            DeckOperation::Identity => "identity",
            DeckOperation::Brightness => "brightness",
            DeckOperation::SetKeyImage => "set_key_image",
            DeckOperation::ClearKey => "clear_key",
            DeckOperation::ClearAll => "clear_all",
            DeckOperation::Sleep => "sleep",
            DeckOperation::SetDial => "set_dial",
            DeckOperation::PollEvents => "poll_events",
            DeckOperation::Reconnect => "reconnect",
        };
        f.write_str(name)
    }
}

/// What went wrong moving bytes to or from a device.
///
/// ```
/// use deck_api::TransportError;
///
/// let error = TransportError::Stalled("the device stopped answering".to_owned());
/// assert_eq!(error.to_string(), "transport stalled: the device stopped answering");
/// ```
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportError {
    /// The device answered with an I/O error.
    #[error("transport I/O failed: {0}")]
    Io(String),

    /// The device stopped answering mid-transfer.
    #[error("transport stalled: {0}")]
    Stalled(String),
}

/// What went wrong doing something to a deck.
///
/// ```
/// use deck_api::{DeckError, DeckOperation};
///
/// let error = DeckError::Unsupported {
///     operation: DeckOperation::Brightness,
/// };
/// assert_eq!(error.to_string(), "brightness is not supported by this deck");
/// ```
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeckError {
    /// The deck does not do this at all.
    ///
    /// This is the error that keeps a second deck honest: a deck without
    /// brightness says so, instead of accepting the call and doing nothing.
    #[error("{operation} is not supported by this deck")]
    Unsupported {
        /// The operation that was refused.
        operation: DeckOperation,
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

    /// The value is outside the range the operation accepts.
    ///
    /// The seam refuses it instead of clamping it: a clamped value is a
    /// request that was quietly changed, and the caller would never know.
    #[error("{operation} value {value} is out of range (expected {min} to {max})")]
    ValueOutOfRange {
        /// The operation whose value was refused.
        operation: DeckOperation,
        /// The value that was offered.
        value: u8,
        /// The lowest value the operation accepts.
        min: u8,
        /// The highest value the operation accepts.
        max: u8,
    },

    /// The dial does not exist on this deck.
    #[error("dial {dial} does not exist (this deck has {dial_count} dials)")]
    DialOutOfRange {
        /// The dial that was asked for.
        dial: u8,
        /// How many dials the deck actually has.
        dial_count: u16,
    },

    /// There is no device to talk to.
    #[error("no device found")]
    DeviceNotFound,

    /// The bytes themselves failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
}

impl DeckError {
    /// The refusal for a value the operation does not accept.
    ///
    /// Every implementation refuses an out-of-range value the same way, so
    /// the message is the same wherever it comes from.
    ///
    /// ```
    /// use deck_api::{DeckError, DeckOperation};
    ///
    /// let error = DeckError::value_out_of_range(DeckOperation::Brightness, 220);
    /// assert_eq!(
    ///     error.to_string(),
    ///     "brightness value 220 is out of range (expected 0 to 100)"
    /// );
    /// ```
    pub fn value_out_of_range(operation: DeckOperation, value: u8) -> Self {
        Self::ValueOutOfRange {
            operation,
            value,
            min: VALUE_MIN,
            max: VALUE_MAX,
        }
    }
}
