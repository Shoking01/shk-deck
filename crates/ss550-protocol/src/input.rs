//! Input report decoding: what the deck sends when a key is touched.
//!
//! Everything here is measured on hardware, not inferred from the vendor
//! libraries. See `docs/troubleshooting/deck-stuck.md`.
//!
//! ## Where input comes from
//!
//! The deck exposes two HID collections under one PID:
//!
//! ```text
//! MI_00  usage_page=0xffa0  usage=0x0001   vendor   <- writes AND button reports
//! MI_01  usage_page=0x0001  usage=0x0006   keyboard <- silent, 0 reports
//! ```
//!
//! Button presses arrive on the **vendor** collection as interrupt IN reports.
//! The keyboard collection never emitted a byte, so presses are *not* delivered
//! to the OS as keypresses.
//!
//! ## Report layout
//!
//! Input reports are **512 bytes and carry no report ID byte**. Output reports
//! are 1025 bytes and *do* start with `0x00`, so the two directions are
//! asymmetric; do not reuse an output buffer here.
//!
//! ```text
//! [0..3]  "ACK"
//! [3..5]  zero
//! [5..7]  "OK"
//! [7..9]  zero
//! [9]     key index, 1..=15, in the READ numbering (see below)
//! [10]    state: 0x01 pressed, 0x00 released
//! ```
//!
//! Measured samples (first 11 bytes of 512):
//!
//! ```text
//! 41 43 4b 00 00 4f 4b 00 00 01 01   press   key 01
//! 41 43 4b 00 00 4f 4b 00 00 01 00   release key 01
//! ```
//!
//! Every click produces **two** reports, press then release, 150-270 ms apart.
//! There is no debouncing in this layer: both edges are reported.
//!
//! ## Two numberings
//!
//! The firmware paints keys and reports keys with **different** start corners:
//!
//! | Corner | Paint (`build_bat`) | Read (`report[9]`) |
//! |---|---|---|
//! | top-left | 11 | 1 |
//! | top-right | 15 | 5 |
//! | bottom-left | 1 | 11 |
//!
//! Both are row-major over the same 3x5 grid; paint starts at the bottom row
//! and read starts at the top row. [`read_index_to_firmware_key`] applies that
//! row flip so callers only ever see the paint numbering, which is the one
//! every other builder in this crate uses.

use crate::error::ProtocolError;

/// Payload size of one input report. The device always sends 512 bytes.
pub const INPUT_REPORT_SIZE: usize = 512;

/// Keys per row in the 3x5 grid.
const COLUMNS: u8 = 5;

/// Rows in the grid.
const ROWS: u8 = 3;

/// Lowest byte offset this decoder reads. Anything shorter cannot be an event.
const MIN_LEN: usize = 11;

/// Whether a key is currently held down or has just been let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonState {
    /// The key was released (`report[10] == 0x00`).
    Released = 0,
    /// The key was pressed (`report[10] == 0x01`).
    Pressed = 1,
}

/// One decoded key edge from an input report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    /// Firmware key in the **paint** numbering, 1-based, 1..=15 — the same
    /// numbering [`crate::build_bat`] and [`crate::build_cle_key`] take.
    pub key: u8,
    /// Which edge this report represents.
    pub state: ButtonState,
}

/// Converts a read-numbered key (1..=15, from the top-left) to the paint
/// numbering (1..=15, from the bottom-left).
///
/// Both are row-major over the same 3x5 grid; only the starting row differs.
/// Returns `None` outside 1..=15.
///
/// ```rust
/// use ss550_protocol::read_index_to_firmware_key;
///
/// // Measured on hardware: the top-left key reads as 1 and paints as 11.
/// assert_eq!(read_index_to_firmware_key(1), Some(11));
/// assert_eq!(read_index_to_firmware_key(5), Some(15));  // top-right
/// assert_eq!(read_index_to_firmware_key(11), Some(1));  // bottom-left
/// assert_eq!(read_index_to_firmware_key(15), Some(5));  // bottom-right
/// // The middle row is unchanged by a row flip.
/// assert_eq!(read_index_to_firmware_key(8), Some(8));
/// assert_eq!(read_index_to_firmware_key(0), None);
/// ```
pub fn read_index_to_firmware_key(read: u8) -> Option<u8> {
    if !(1..=ROWS * COLUMNS).contains(&read) {
        return None;
    }
    let zero = read - 1;
    let row_from_top = zero / COLUMNS;
    let column = zero % COLUMNS;
    let row_from_bottom = (ROWS - 1) - row_from_top;
    Some(row_from_bottom * COLUMNS + column + 1)
}

/// Decodes one interrupt IN report into a key edge.
///
/// Accepts any slice of at least [`MIN_LEN`] bytes whose header matches; the
/// device sends [`INPUT_REPORT_SIZE`]. The header is validated strictly so an
/// unrecognised report surfaces as an error instead of being misread as a
/// keypress.
///
/// ```rust
/// use ss550_protocol::{parse_input_report, ButtonState};
///
/// // Measured: press of the top-left key.
/// let mut report = [0u8; 512];
/// report[0..3].copy_from_slice(b"ACK");
/// report[5..7].copy_from_slice(b"OK");
/// report[9] = 1;
/// report[10] = 1;
///
/// let event = parse_input_report(&report).expect("valid event");
/// assert_eq!(event.key, 11); // paint numbering: top-left is key 11
/// assert_eq!(event.state, ButtonState::Pressed);
/// ```
pub fn parse_input_report(report: &[u8]) -> Result<InputEvent, ProtocolError> {
    if report.len() < MIN_LEN {
        return Err(ProtocolError::MalformedInputReport(format!(
            "expected at least {MIN_LEN} bytes, got {}",
            report.len()
        )));
    }

    if &report[0..3] != b"ACK" {
        return Err(ProtocolError::MalformedInputReport(format!(
            "expected \"ACK\" at [0..3], got {:02x?}",
            &report[0..3]
        )));
    }

    if &report[5..7] != b"OK" {
        return Err(ProtocolError::MalformedInputReport(format!(
            "expected \"OK\" at [5..7], got {:02x?}",
            &report[5..7]
        )));
    }

    let read_key = report[9];
    let key = read_index_to_firmware_key(read_key).ok_or_else(|| {
        ProtocolError::MalformedInputReport(format!("key index must be 1..=15, got {read_key}"))
    })?;

    let state = match report[10] {
        0 => ButtonState::Released,
        1 => ButtonState::Pressed,
        other => {
            return Err(ProtocolError::MalformedInputReport(format!(
                "state must be 0 or 1, got {other}"
            )));
        }
    };

    Ok(InputEvent { key, state })
}

/// Whether a report carries a key edge.
///
/// Every report the deck sends wears the same envelope — `"ACK"` at `[0..3]`
/// and `"OK"` at `[5..7]` — so a reply to a command and a key press look alike
/// until the key byte is read. A command reply (the generic ACK, the `TF`
/// space reply, …) carries key `0`, which is not a key; only a report with a
/// key in `1..=15` and a state of 0 or 1 is an edge.
///
/// A reader polling this endpoint must skip the other replies: the driver
/// never reads them after a write, so they are still queued when a poll
/// happens.
///
/// ```rust
/// use ss550_protocol::is_key_edge;
///
/// let mut reply = [0u8; 512];
/// reply[0..3].copy_from_slice(b"ACK");
/// reply[5..7].copy_from_slice(b"OK");
/// assert!(!is_key_edge(&reply)); // generic ACK: key 0, no key attached
///
/// reply[9] = 11; // read numbering: top-left
/// reply[10] = 1; // pressed
/// assert!(is_key_edge(&reply));
/// ```
pub fn is_key_edge(report: &[u8]) -> bool {
    report.len() >= MIN_LEN
        && &report[0..3] == b"ACK"
        && &report[5..7] == b"OK"
        && read_index_to_firmware_key(report[9]).is_some()
        && matches!(report[10], 0 | 1)
}

#[cfg(test)]
mod key_edge_tests {
    use super::*;

    #[test]
    fn a_command_reply_is_not_a_key_edge() {
        let mut reply = [0u8; INPUT_REPORT_SIZE];
        reply[0..3].copy_from_slice(b"ACK");
        reply[5..7].copy_from_slice(b"OK");
        assert!(!is_key_edge(&reply), "a generic ACK carries key 0");

        reply[9] = 1;
        reply[10] = 1;
        assert!(is_key_edge(&reply), "key 1 pressed is an edge");

        reply[10] = 7;
        assert!(!is_key_edge(&reply), "state must be 0 or 1");

        reply[10] = 1;
        reply[9] = 0;
        assert!(!is_key_edge(&reply), "key must be 1..=15");

        reply[9] = 16;
        assert!(!is_key_edge(&reply), "key must be 1..=15");
    }

    #[test]
    fn a_short_or_unframed_report_is_not_an_edge() {
        assert!(!is_key_edge(&[]));
        assert!(!is_key_edge(&[0u8; 10]));
        let mut unframed = [0u8; INPUT_REPORT_SIZE];
        unframed[9] = 1;
        unframed[10] = 1;
        assert!(!is_key_edge(&unframed), "the ACK/OK envelope is required");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact prefix measured on hardware for a press of the top-left key.
    const MEASURED_TOP_LEFT_PRESS: [u8; 16] = [
        0x41, 0x43, 0x4b, 0x00, 0x00, 0x4f, 0x4b, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];

    fn report_with(read_key: u8, state: u8) -> Vec<u8> {
        let mut r = vec![0u8; INPUT_REPORT_SIZE];
        r[0..3].copy_from_slice(b"ACK");
        r[5..7].copy_from_slice(b"OK");
        r[9] = read_key;
        r[10] = state;
        r
    }

    #[test]
    fn decodes_the_measured_top_left_press() {
        // Golden vector copied from a live run; do not "clean it up".
        let event = parse_input_report(&MEASURED_TOP_LEFT_PRESS).expect("valid");
        assert_eq!(event.key, 11);
        assert_eq!(event.state, ButtonState::Pressed);
    }

    #[test]
    fn decodes_press_and_release_as_two_events() {
        let press = parse_input_report(&report_with(1, 1)).expect("valid");
        let release = parse_input_report(&report_with(1, 0)).expect("valid");
        assert_eq!(press.state, ButtonState::Pressed);
        assert_eq!(release.state, ButtonState::Released);
        // Same physical key on both edges.
        assert_eq!(press.key, release.key);
    }

    #[test]
    fn every_read_index_maps_to_a_distinct_firmware_key() {
        // A duplicated or missing target would silently make two buttons share
        // an action, or make one unreachable.
        let mut seen = [false; 16];
        for read in 1..=15u8 {
            let fw = read_index_to_firmware_key(read).expect("in range");
            assert!((1..=15).contains(&fw), "read {read} -> fw {fw}");
            assert!(!seen[fw as usize], "read {read} collides on fw {fw}");
            seen[fw as usize] = true;
        }
        assert!(seen[1..].iter().all(|&s| s), "every firmware key reachable");
    }

    #[test]
    fn row_flip_matches_the_three_measured_corners() {
        // These three were confirmed one press at a time on hardware.
        assert_eq!(read_index_to_firmware_key(1), Some(11), "top-left");
        assert_eq!(read_index_to_firmware_key(5), Some(15), "top-right");
        assert_eq!(read_index_to_firmware_key(11), Some(1), "bottom-left");
        // Middle row is its own mirror: a row flip must leave it alone.
        for read in 6..=10u8 {
            assert_eq!(read_index_to_firmware_key(read), Some(read));
        }
    }

    #[test]
    fn rejects_a_report_that_is_not_an_ack() {
        let mut r = report_with(1, 1);
        r[0] = b'X';
        let err = parse_input_report(&r).expect_err("must reject");
        assert!(err.to_string().contains("ACK"), "{err}");
    }

    #[test]
    fn rejects_a_report_without_ok() {
        let mut r = report_with(1, 1);
        r[6] = b'X';
        let err = parse_input_report(&r).expect_err("must reject");
        assert!(err.to_string().contains("OK"), "{err}");
    }

    #[test]
    fn rejects_out_of_range_key_and_state() {
        assert!(parse_input_report(&report_with(0, 1)).is_err());
        assert!(parse_input_report(&report_with(16, 1)).is_err());
        assert!(parse_input_report(&report_with(1, 2)).is_err());
    }

    #[test]
    fn rejects_a_truncated_report() {
        let err = parse_input_report(&MEASURED_TOP_LEFT_PRESS[..10]).expect_err("must reject");
        assert!(err.to_string().contains("at least"), "{err}");
    }
}
