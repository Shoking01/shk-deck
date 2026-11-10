//! Property-based tests: builders must be panic-free on arbitrary input
//! and preserve every observable effect (AGENTS.md fuzzing requirement).

use proptest::prelude::*;
use ss550_protocol::{
    build_bat, build_cle_key, build_image_chunk, build_lig, build_mod, CHUNK_SIZE, REPORT_SIZE,
};

proptest! {
    #[test]
    fn lig_is_panic_free_and_byte_correct(v in any::<u8>()) {
        let r = build_lig(v);
        if v > 100 {
            prop_assert!(r.is_err(), "brightness {} must be rejected", v);
        } else {
            let p = r.expect("brightness in 0..100 is valid");
            prop_assert_eq!(p.as_bytes().len(), REPORT_SIZE);
            prop_assert_eq!(p.as_bytes()[11], v);
            prop_assert_eq!(&p.as_bytes()[1..4], b"CRT");
            prop_assert_eq!(&p.as_bytes()[6..9], b"LIG");
        }
    }

    #[test]
    fn bat_is_panic_free_and_preserves_fields(size in any::<usize>(), key in any::<u8>()) {
        let r = build_bat(size, key);
        if !(1..=15).contains(&key) || size > 65535 {
            prop_assert!(r.is_err(), "size={} key={} must be rejected", size, key);
        } else {
            let p = r.expect("valid size and key");
            prop_assert_eq!(p.as_bytes().len(), REPORT_SIZE);
            prop_assert_eq!(&p.as_bytes()[6..9], b"BAT");
            // BE16 length at wire [11..12], key at [13].
            prop_assert_eq!(p.as_bytes()[11] as usize * 256 + p.as_bytes()[12] as usize, size);
            prop_assert_eq!(p.as_bytes()[13], key);
        }
    }

    #[test]
    fn chunk_is_panic_free_and_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let r = build_image_chunk(&bytes);
        if bytes.len() > CHUNK_SIZE {
            prop_assert!(r.is_err(), "chunk {} bytes must be rejected", bytes.len());
        } else {
            let p = r.expect("chunk is at most 1024 bytes");
            prop_assert_eq!(p.as_bytes().len(), REPORT_SIZE);
            prop_assert_eq!(p.as_bytes()[0], 0x00);
            prop_assert_eq!(&p.as_bytes()[1..1 + bytes.len()], bytes.as_slice());
            prop_assert!(p.as_bytes()[1 + bytes.len()..].iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn cle_key_is_validated(v in any::<u8>()) {
        let r = build_cle_key(v);
        if (1..=15).contains(&v) {
            let p = r.expect("key in 1..15 is valid");
            prop_assert_eq!(p.as_bytes()[12], v);
        } else {
            prop_assert!(r.is_err(), "key {} must be rejected", v);
        }
    }

    #[test]
    fn mod_is_ascii_digit_or_error(v in any::<u8>()) {
        let r = build_mod(v);
        if v > 9 {
            prop_assert!(r.is_err());
        } else {
            let p = r.expect("mode 0..9 is valid");
            prop_assert_eq!(p.as_bytes()[11], b'0' + v);
        }
    }
}
