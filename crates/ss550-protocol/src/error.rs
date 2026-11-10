//! Protocol errors for SS550 HID pack building.

use thiserror::Error;

/// Errors that can occur while building SS550 HID packs.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("Brightness must be 0-100, got {0}")]
    InvalidBrightness(u8),

    #[error("Firmware key must be 1-15, got {0}")]
    InvalidKey(u8),

    #[error("Image size {0} exceeds BE16 maximum (65535)")]
    SizeTooLarge(usize),

    #[error("Image chunk size {0} exceeds maximum (1024)")]
    ChunkTooLarge(usize),

    #[error("Mode must be 0-9, got {0}")]
    InvalidMode(u8),

    #[error("Malformed input report: {0}")]
    MalformedInputReport(String),
}
