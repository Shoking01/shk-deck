//! Error types for ss550-driver.

use ss550_protocol::ProtocolError;
use thiserror::Error;

/// Errors that can occur when communicating with the SS550.
#[derive(Debug, Error)]
pub enum Ss550Error {
    #[error("Device not found with VID 0x{vid:04X} PID 0x{pid:04X}")]
    DeviceNotFound { vid: u16, pid: u16 },

    #[error("Failed to open HID device: {0}")]
    HidOpenError(String),

    #[error("HID I/O error: {0}")]
    HidIoError(String),

    #[error("Invalid report size, expected {expected} bytes, got {actual}")]
    InvalidReportSize { expected: usize, actual: usize },

    #[error("Image decode error: {0}")]
    ImageDecodeError(String),

    #[error("Image encode error: {0}")]
    ImageEncodeError(String),

    #[error("Key index {0} out of range (0-14)")]
    KeyOutOfRange(u8),

    #[error("Brightness must be 0-100, got {0}")]
    InvalidBrightness(u8),

    #[error("JPEG payload exceeds maximum size ({max} bytes), got {size} bytes")]
    PayloadTooLarge { max: usize, size: usize },

    #[error("Device not initialized, call `Device::initialize` first")]
    NotInitialized,

    #[error("Concurrent access error: another task is using the device")]
    ConcurrentAccess,

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Malformed input report from the deck: {0}")]
    MalformedInputReport(String),

    #[error(
        "Device stopped accepting reports (USB endpoint halted): {detail}. Power \
         cycle the deck: unplug USB for 60 s, then reconnect on another port. A \
         5 s replug is not enough. See docs/troubleshooting/deck-stuck.md"
    )]
    TransportStalled { detail: String },
}

/// hidapi reports these Windows errors once the interrupt OUT endpoint is
/// halted: `0x1F` (`ERROR_GEN_FAILURE`, the report is refused outright) and
/// `0x3E5` (`ERROR_IO_PENDING`, the transfer never completes inside hidapi's
/// fixed 1 s wait). The crate only exposes an error message, so detection is
/// textual; anything else stays a plain I/O error.
pub(crate) fn is_stalled_transport(message: &str) -> bool {
    message.contains("0x000003E5") || message.contains("0x0000001F")
}

impl From<ProtocolError> for Ss550Error {
    fn from(err: ProtocolError) -> Self {
        match err {
            ProtocolError::InvalidBrightness(b) => Self::InvalidBrightness(b),
            ProtocolError::InvalidKey(k) => Self::KeyOutOfRange(k),
            ProtocolError::SizeTooLarge(size) => Self::PayloadTooLarge {
                max: ss550_protocol::MAX_IMAGE_SIZE,
                size,
            },
            ProtocolError::ChunkTooLarge(size) => Self::PayloadTooLarge {
                max: ss550_protocol::CHUNK_SIZE,
                size,
            },
            ProtocolError::InvalidMode(m) => Self::Protocol(format!("Invalid mode {m}")),
            ProtocolError::MalformedInputReport(m) => Self::MalformedInputReport(m),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_stalled_transport, Ss550Error};

    #[test]
    fn detects_halted_endpoint_errors() {
        assert!(is_stalled_transport(
            "hidapi error: hid_write/WaitForSingleObject: (0x000003E5) \
             Overlapped I/O operation is in progress."
        ));
        assert!(is_stalled_transport(
            "hidapi error: hid_write/GetOverlappedResult: (0x0000001F) \
             A device attached to the system is not functioning."
        ));
    }

    #[test]
    fn leaves_other_errors_alone() {
        assert!(!is_stalled_transport(
            "hidapi error: hid_open: No such device"
        ));
        assert!(!is_stalled_transport(""));
    }

    /// The two halted-endpoint codes mean different things, so the raw message
    /// must survive into the error. Losing it made a live incident ambiguous:
    /// `0x1F` is an outright refusal, `0x3E5` is a transfer that never completes.
    #[test]
    fn keeps_the_underlying_transport_message() {
        let refused = Ss550Error::TransportStalled {
            detail: "hid_write/GetOverlappedResult: (0x0000001F)".to_string(),
        };
        let pending = Ss550Error::TransportStalled {
            detail: "hid_write/WaitForSingleObject: (0x000003E5)".to_string(),
        };

        let refused = refused.to_string();
        let pending = pending.to_string();

        assert!(refused.contains("0x0000001F"));
        assert!(pending.contains("0x000003E5"));
        assert!(pending.contains("60 s"));
    }
}
