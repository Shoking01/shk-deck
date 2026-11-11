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
//!   on bytes wrapped in a valid `ACK`/`OK` envelope (at every length from the
//!   minimum upwards), and on an oversized buffer.
//! - `is_key_edge` on the same inputs.
//! - `read_index_to_firmware_key` with every value the fuzzer can produce.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ss550_protocol::{
    is_key_edge, parse_input_report, read_index_to_firmware_key, INPUT_REPORT_SIZE, REPORT_SIZE,
};

/// Number of payload bytes of a report that are not the envelope.
const PAYLOAD_OFFSET: usize = 11;

/// How many lengths the enveloped sweep tries per input.
///
/// One allocation per length would make the cost of an input quadratic in its
/// length, and a fuzzer buys inputs with time: the sweep takes a bounded
/// number of samples across the range instead of every length in it.
const MAX_SWEEP_LENGTHS: usize = 64;

/// Builds a report of `len` bytes with a valid `ACK`/`OK` envelope around `data`.
///
/// Random bytes never spell `ACK` at the front, so without this the fuzzer
/// would only ever reach the envelope check. Here the key index and the state
/// byte are driven with adversarial values instead.
///
/// `len` must be at least [`PAYLOAD_OFFSET`]: that is what keeps the key
/// index at `[9]` and the state at `[10]` inside the buffer.
fn frame_report(data: &[u8], len: usize) -> Vec<u8> {
    let mut report = vec![0u8; len];
    report[0..3].copy_from_slice(b"ACK");
    report[5..7].copy_from_slice(b"OK");

    let mut rest = data;
    if let Some(&state) = rest.first() {
        report[10] = state;
        rest = &rest[1..];
    }
    if let Some(&key) = rest.first() {
        report[9] = key;
        rest = &rest[1..];
    }
    let n = rest.len().min(len - PAYLOAD_OFFSET);
    report[PAYLOAD_OFFSET..PAYLOAD_OFFSET + n].copy_from_slice(&rest[..n]);
    report
}

fuzz_target!(|data: &[u8]| {
    // The report exactly as a hostile or broken device would send it.
    let _ = parse_input_report(data);
    let _ = is_key_edge(data);

    // Every prefix of it: an off-by-one in the length guard shows up here.
    for len in 0..=data.len() {
        let _ = parse_input_report(&data[..len]);
        let _ = is_key_edge(&data[..len]);
    }

    // Reports that still carry the envelope, at every length from the
    // minimum upwards. The prefix sweep cuts random bytes, which essentially
    // never spell ACK, and the full-length framed input below is always long,
    // so without this the only inputs that reach the key and state fields
    // are long ones: an off-by-one at the state byte, which needs a report
    // that is just barely long enough, would never be exercised.
    let max_len = data.len() + PAYLOAD_OFFSET;
    let step = (max_len - PAYLOAD_OFFSET)
        .div_ceil(MAX_SWEEP_LENGTHS)
        .max(1);
    let mut len = PAYLOAD_OFFSET;
    while len <= max_len {
        let short = frame_report(data, len);
        let _ = parse_input_report(&short);
        let _ = is_key_edge(&short);
        len += step;
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
