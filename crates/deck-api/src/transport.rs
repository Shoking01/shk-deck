//! The transport trait: how the bytes travel.
//!
//! A transport knows nothing about decks. It moves reports and reports what
//! went wrong, and everything above it — the protocol, the key numbering, the
//! image format — belongs to the deck that sits on top.

use std::time::Duration;

use crate::error::TransportError;

/// How the bytes reach a device.
///
/// The three calls are the whole surface a deck needs: write one report, read
/// an input report with a timeout, read a control report without one.
///
/// The timeout is a [`Duration`], the same unit the deck layer uses, so no
/// implementation has to translate between two conventions.
///
/// ```
/// use std::time::Duration;
///
/// use deck_api::Transport;
///
/// fn reads_with_a_deadline(transport: &mut dyn Transport) -> usize {
///     let mut buffer = [0_u8; 64];
///     transport.read_input(&mut buffer, Duration::from_millis(20)).unwrap_or(0)
/// }
/// ```
pub trait Transport {
    /// Writes one report to the device and returns how many bytes went out.
    ///
    /// A short write is not a failure to hide: it is reported, because the
    /// caller is the only one that can decide whether the report matters.
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError>;

    /// Reads one input report into `buf`, waiting up to `timeout`.
    ///
    /// Returns how many bytes were read. **Zero means the wait ended with
    /// nothing waiting** — that is not an error, it is the normal answer to
    /// "nobody pressed anything". A device that stopped answering is a
    /// different thing, and it is reported as
    /// [`TransportError::Stalled`].
    fn read_input(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError>;

    /// Reads one control report into `buf`, without waiting.
    ///
    /// Returns how many bytes were read. **Zero means no control report was
    /// pending**, which is the normal answer most of the time.
    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;
}
