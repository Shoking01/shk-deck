//! The parts of the fuzz harness that can be tested without a fuzzer.
//!
//! The target itself lives in `fuzz_targets/decode_input_report.rs` and
//! cannot be unit-tested — a libFuzzer target is a binary with `#![no_main]`.
//! The two decisions that shape the enveloped inputs therefore live here, so
//! they can be pinned by ordinary tests:
//!
//! ```text
//! cargo test --manifest-path fuzz/Cargo.toml --lib
//! ```
//!
//! The target keeps two other steps of its own, which are not shaped here:
//! the prefix sweep and the full-length envelope.

/// Number of payload bytes of a report that are not the envelope.
///
/// The decoder reads the key index at [`KEY_INDEX_OFFSET`] and the state at
/// [`STATE_OFFSET`], so a report shorter than this cannot carry them.
pub const PAYLOAD_OFFSET: usize = 11;

/// Where the key index sits in a report.
pub const KEY_INDEX_OFFSET: usize = 9;

/// Where the state byte sits in a report.
pub const STATE_OFFSET: usize = 10;

/// How many lengths the sweep always visits, starting at the minimum.
///
/// An off-by-one at the state byte only shows in a report that is barely
/// long enough, so those lengths are never sampled away.
pub const BAND_LENGTHS: usize = 16;

/// How many lengths the sweep samples from the rest of the range.
///
/// One report per length would make the cost of an input quadratic in its
/// length, and a fuzzer buys inputs with time.
pub const MAX_SWEEP_LENGTHS: usize = 64;

/// Builds a report of `len` bytes with a valid `ACK`/`OK` envelope around `data`.
///
/// Random bytes never spell `ACK` at the front, so without this the fuzzer
/// would only ever reach the envelope check. Here the key index and the state
/// byte are driven with adversarial values instead.
///
/// `len` must be at least [`PAYLOAD_OFFSET`]: that is what keeps the key
/// index and the state byte inside the buffer. The precondition is
/// enforced, so a caller that gets it wrong gets a clear message instead of
/// an index panic — in a harness, libFuzzer reports a panic as a crash,
/// which is the one failure this target must not confuse with a decoder bug.
pub fn frame_report(data: &[u8], len: usize) -> Vec<u8> {
    assert!(
        len >= PAYLOAD_OFFSET,
        "an enveloped report needs at least {PAYLOAD_OFFSET} bytes, got {len}"
    );

    let mut report = vec![0u8; len];
    report[0..3].copy_from_slice(b"ACK");
    report[5..7].copy_from_slice(b"OK");

    let mut rest = data;
    if let Some(&state) = rest.first() {
        report[STATE_OFFSET] = state;
        rest = &rest[1..];
    }
    if let Some(&key) = rest.first() {
        report[KEY_INDEX_OFFSET] = key;
        rest = &rest[1..];
    }
    let n = rest.len().min(len - PAYLOAD_OFFSET);
    report[PAYLOAD_OFFSET..PAYLOAD_OFFSET + n].copy_from_slice(&rest[..n]);
    report
}

/// The report lengths the sweep feeds to the decoder, for an input of
/// `input_len` bytes.
///
/// The first [`BAND_LENGTHS`] lengths, starting at the minimum, are always
/// visited: that is where an off-by-one at the state byte shows. The rest of
/// the range is sampled, at most [`MAX_SWEEP_LENGTHS`] times, so the cost of
/// an input stays bounded.
///
/// The band does not depend on the size of the input: a report is zero
/// padded, so a report longer than the input is still a shape worth feeding.
pub fn swept_lengths(input_len: usize) -> Vec<usize> {
    let max_len = input_len.saturating_add(PAYLOAD_OFFSET);
    let band_end = PAYLOAD_OFFSET + BAND_LENGTHS - 1;

    let mut lengths = Vec::with_capacity(BAND_LENGTHS + MAX_SWEEP_LENGTHS);

    for len in PAYLOAD_OFFSET..=band_end {
        lengths.push(len);
    }

    let step = (max_len.saturating_sub(band_end) / MAX_SWEEP_LENGTHS).max(1);
    let mut len = band_end + 1;
    let mut taken = 0;
    while len <= max_len && taken < MAX_SWEEP_LENGTHS {
        lengths.push(len);
        taken += 1;
        len += step;
    }

    lengths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inputs of every interesting size: empty, one byte, shorter than the
    /// envelope, exactly at the end of the band, one past it, and far beyond.
    const INPUT_LENGTHS: [usize; 6] = [0, 1, 10, BAND_LENGTHS, BAND_LENGTHS + 1, 4096];

    #[test]
    fn every_length_of_the_band_is_always_visited() {
        for input_len in INPUT_LENGTHS {
            let lengths = swept_lengths(input_len);
            for offset in 0..BAND_LENGTHS {
                let wanted = PAYLOAD_OFFSET + offset;
                assert!(
                    lengths.contains(&wanted),
                    "length {wanted} is missing for an input of {input_len}"
                );
            }
        }
    }

    #[test]
    fn the_sweep_visits_lengths_above_the_band() {
        let band_end = PAYLOAD_OFFSET + BAND_LENGTHS - 1;
        let lengths = swept_lengths(4096);
        assert!(
            lengths.iter().any(|length| *length > band_end),
            "no length above the band was visited"
        );
    }

    #[test]
    fn the_sampled_lengths_stay_within_the_input() {
        // The band is visited even when it is longer than the input allows
        // — a report is zero padded, so a longer report is still a shape
        // worth feeding. The sampled lengths, on the other hand, never exceed
        // what the input can fill.
        let band_end = PAYLOAD_OFFSET + BAND_LENGTHS - 1;
        for input_len in INPUT_LENGTHS {
            let max_len = input_len.saturating_add(PAYLOAD_OFFSET);
            for length in swept_lengths(input_len) {
                if length > band_end {
                    assert!(
                        length <= max_len,
                        "{length} is longer than the input of {input_len} allows"
                    );
                }
            }
        }
    }

    #[test]
    fn the_sweep_never_exceeds_its_bound() {
        for input_len in INPUT_LENGTHS {
            let lengths = swept_lengths(input_len);
            assert!(
                lengths.len() <= BAND_LENGTHS + MAX_SWEEP_LENGTHS,
                "an input of {input_len} produced {} lengths",
                lengths.len()
            );
        }
    }

    #[test]
    fn no_length_is_below_the_minimum() {
        for input_len in INPUT_LENGTHS {
            for length in swept_lengths(input_len) {
                assert!(length >= PAYLOAD_OFFSET, "{length} is too short");
            }
        }
    }

    #[test]
    fn the_sweep_never_repeats_a_length() {
        for input_len in INPUT_LENGTHS {
            let lengths = swept_lengths(input_len);
            let mut sorted = lengths.clone();
            sorted.dedup();
            assert_eq!(sorted.len(), lengths.len(), "a length was visited twice");
        }
    }

    #[test]
    fn the_envelope_fits_in_the_report() {
        let report = frame_report(&[1, 2, 3], PAYLOAD_OFFSET);
        assert_eq!(report.len(), PAYLOAD_OFFSET);
        assert_eq!(&report[0..3], b"ACK");
        assert_eq!(&report[5..7], b"OK");
        assert_eq!(report[KEY_INDEX_OFFSET], 2);
        assert_eq!(report[STATE_OFFSET], 1);
    }

    #[test]
    fn a_report_shorter_than_the_envelope_is_refused() {
        // The harness must fail loudly on its own mistake, but not with a
        // panic message in the test output.
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(|| frame_report(&[], PAYLOAD_OFFSET - 1));
        std::panic::set_hook(hook);
        assert!(result.is_err(), "a too short report was accepted");
    }
}
