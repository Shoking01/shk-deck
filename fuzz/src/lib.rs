//! The parts of the fuzz harness that can be tested without a fuzzer.
//!
//! The target itself lives in `fuzz_targets/decode_input_report.rs` and
//! cannot be unit-tested — a libFuzzer target is a binary with `#![no_main]`.
//! Everything that decides *which* inputs the decoder sees lives here, so it
//! can be pinned by ordinary tests:
//!
//! ```text
//! cargo test --manifest-path fuzz/Cargo.toml
//! ```

/// Number of payload bytes of a report that are not the envelope.
///
/// The decoder reads the key index at `[9]` and the state at `[10]`, so a
/// report shorter than this cannot carry them.
pub const PAYLOAD_OFFSET: usize = 11;

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
/// index at `[9]` and the state at `[10]` inside the buffer. The precondition
/// is enforced, so a caller that gets it wrong gets a clear message instead
/// of an index panic — in a harness, libFuzzer reports a panic as a crash,
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
    let max_len = input_len + PAYLOAD_OFFSET;
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

    /// Inputs of every interesting size: empty, shorter than the envelope,
    /// exactly at the bound, and far past it.
    const INPUT_LENGTHS: [usize; 6] = [0, 1, 10, 64, 65, 4096];

    #[test]
    fn the_minimum_length_is_always_visited() {
        for input_len in INPUT_LENGTHS {
            assert!(
                swept_lengths(input_len).contains(&PAYLOAD_OFFSET),
                "the minimum length is missing for an input of {input_len}"
            );
        }
    }

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
    fn the_sweep_is_deterministic() {
        assert_eq!(swept_lengths(300), swept_lengths(300));
    }

    #[test]
    fn the_envelope_fits_in_the_report() {
        let report = frame_report(&[1, 2, 3], PAYLOAD_OFFSET);
        assert_eq!(report.len(), PAYLOAD_OFFSET);
        assert_eq!(&report[0..3], b"ACK");
        assert_eq!(&report[5..7], b"OK");
        assert_eq!(report[9], 2);
        assert_eq!(report[10], 1);
    }
}
