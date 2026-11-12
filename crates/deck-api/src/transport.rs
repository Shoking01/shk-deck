//! The transport trait: how the bytes travel.
//!
//! A transport knows nothing about decks. It moves reports and reports what
//! went wrong, and everything above it — the protocol, the key numbering, the
//! image format — belongs to the deck that sits on top.

use crate::error::TransportError;

/// How the bytes reach a device.
///
/// The three calls are the whole surface a deck needs: write one report, read
/// an input report with a timeout, read a control report without one.
pub trait Transport {
    /// Writes one report to the device and returns how many bytes went out.
    ///
    /// A short write is not a failure to hide: it is reported, because the
    /// caller is the only one that can decide whether the report matters.
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError>;

    /// Reads one input report into `buf`, waiting up to `timeout_ms`
    /// milliseconds.
    ///
    /// Returns how many bytes were read; zero means the timeout expired with
    /// nothing waiting.
    fn read_input(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, TransportError>;

    /// Reads one control report into `buf`, without waiting.
    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;
}
