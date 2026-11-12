//! The one [`Transport`] that talks to a real HID handle.
//!
//! Every byte this crate sends or receives passes through this file and
//! nowhere else: the protocol, the key numbering, the image format and the
//! verified-command gate all live above it, which is why they can be tested
//! over a fake transport with no device attached.
//!
//! The write below is the crate's **only** physical write. The guard in
//! `connection.rs` (`every_write_goes_through_the_gated_funnel`) scans the
//! whole crate for it, so a second writer added anywhere fails the build.

use std::time::Duration;

use deck_api::{Transport, TransportError};
use hidapi::HidDevice;

use crate::error::is_stalled_transport;

/// A [`Transport`] over an already-open device handle.
pub(crate) struct HidTransport {
    device: HidDevice,
}

impl HidTransport {
    /// Wraps an open handle. Opening and settling stay in `connection.rs`.
    pub(crate) fn new(device: HidDevice) -> Self {
        Self { device }
    }
}

impl Transport for HidTransport {
    /// Writes one report over interrupt OUT.
    ///
    /// This is the reference implementation's path. Control `Set_Report`
    /// (`send_output_report`) returns success but the firmware ignores it, so
    /// it must never be used here: a silent no-op is worse than a loud error.
    ///
    /// A halted endpoint is classified here, where the error is produced, and
    /// reported as [`TransportError::Stalled`] so the driver can name the
    /// recovery in `docs/troubleshooting/deck-stuck.md`.
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        self.device.write(bytes).map_err(|error| {
            let message = error.to_string();
            if is_stalled_transport(&message) {
                TransportError::Stalled(message)
            } else {
                TransportError::Io(message)
            }
        })
    }

    /// Reads one interrupt IN report, waiting up to `timeout`.
    ///
    /// Zero bytes is the normal "nobody pressed anything" answer and stays
    /// `Ok(0)`; only a real I/O failure becomes an error.
    fn read_input(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        self.device
            .read_timeout(buf, duration_to_timeout_ms(timeout))
            .map_err(|error| TransportError::Io(error.to_string()))
    }

    /// Reads one control report, without waiting.
    ///
    /// The SS550 serves its identity from a static report, so this read is the
    /// only way to ask the deck anything without writing a command — and
    /// writing `VER` is exactly what halts the endpoint.
    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        self.device
            .get_input_report(buf)
            .map_err(|error| TransportError::Io(error.to_string()))
    }
}

/// Clamps a [`Duration`] into hidapi's millisecond `i32` timeout.
fn duration_to_timeout_ms(wait: Duration) -> i32 {
    let ms = wait.as_millis();
    if ms > i32::MAX as u128 {
        i32::MAX
    } else {
        ms as i32
    }
}

#[cfg(test)]
mod tests {
    use super::duration_to_timeout_ms;
    use std::time::Duration;

    #[test]
    fn a_timeout_is_carried_into_hidapi_unchanged() {
        assert_eq!(duration_to_timeout_ms(Duration::ZERO), 0);
        assert_eq!(duration_to_timeout_ms(Duration::from_millis(250)), 250);
    }

    #[test]
    fn a_timeout_too_large_for_hidapi_is_clamped_instead_of_wrapping() {
        // A wrapping cast would turn "wait forever" into "wait not at all".
        assert_eq!(duration_to_timeout_ms(Duration::MAX), i32::MAX);
    }
}
