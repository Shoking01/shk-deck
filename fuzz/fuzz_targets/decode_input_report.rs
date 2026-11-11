//! Fuzz target: the SS550 input-report decoder must never panic.
//!
//! The deck is a physical USB device. A malformed report that makes the
//! decoder panic would take the whole host down with it, and a decoder that
//! trusts the device is precisely what broken — or hostile — firmware can
//! attack. This target therefore asserts **nothing** about the result of a
//! decode: rejecting a malformed report is a correct outcome, panicking is
//! not. Only "no panic" is a pass.
//!
//! What it covers:
//!
//! - `parse_input_report` on arbitrary bytes, on every prefix of those bytes,
//!   on bytes wrapped in a valid `ACK`/`OK` envelope (at every length just
//!   above the minimum, and at a bounded sample of the rest), and on an
//!   oversized buffer.
//! - `is_key_edge` on the same inputs.
//! - `read_index_to_firmware_key` with every value the fuzzer can produce.
//!
//! Which lengths the envelope sweep visits is decided in `ss550_fuzz`, where
//! ordinary unit tests pin it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ss550_fuzz::{frame_report, swept_lengths};
use ss550_protocol::{
    is_key_edge, parse_input_report, read_index_to_firmware_key, INPUT_REPORT_SIZE, REPORT_SIZE,
};

fuzz_target!(|data: &[u8]| {
    // The report exactly as a hostile or broken device would send it.
    let _ = parse_input_report(data);
    let _ = is_key_edge(data);

    // Every prefix of it: an off-by-one in the length guard shows up here.
    for len in 0..=data.len() {
        let _ = parse_input_report(&data[..len]);
        let _ = is_key_edge(&data[..len]);
    }

    // Reports that still carry the envelope. The prefix sweep cuts random
    // bytes, which essentially never spell ACK, and the full-length framed
    // input below is always long, so without this the only inputs that reach
    // the key and state fields are long ones.
    for len in swept_lengths(data.len()) {
        let report = frame_report(data, len);
        let _ = parse_input_report(&report);
        let _ = is_key_edge(&report);
    }

    // The same bytes inside a full-length valid envelope, so the key index
    // and the state byte are reached with adversarial values.
    let framed = frame_report(data, INPUT_REPORT_SIZE);
    let _ = parse_input_report(&framed);
    let _ = is_key_edge(&framed);

    // A report in a transport-sized buffer: longer than the 512 bytes the
    // deck sends, so a decoder that assumed the exact size would trip here.
    let mut oversized = [0u8; REPORT_SIZE];
    let n = data.len().min(REPORT_SIZE);
    oversized[..n].copy_from_slice(&data[..n]);
    let _ = parse_input_report(&oversized);
    let _ = is_key_edge(&oversized);

    // The key-number mapping, with every value the fuzzer can produce.
    for &byte in data {
        let _ = read_index_to_firmware_key(byte);
    }
});
