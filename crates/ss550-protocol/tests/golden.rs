//! Byte-exact golden vectors for SS550 HID packs.
//!
//! Wire layout reminder (wet = report incl. report ID at [0]):
//! [0]=0x00, [1..4]="CRT", [4]=0, [5]=0, [6..]=tag, fields follow tag.
//! Every pack is padded with zeros to 1025 bytes.

use ss550_protocol::{
    build_bat, build_cle_all, build_cle_key, build_dis, build_han_bare, build_lig, build_mod,
    build_sleep, build_stp, build_ver, CHUNK_SIZE, REPORT_SIZE,
};

use ss550_protocol::{build_connect, build_image_chunks};

#[test]
fn dis_is_byte_exact() {
    let pack = build_dis();
    assert_eq!(pack.as_bytes().len(), REPORT_SIZE);
    assert_eq!(
        &pack.as_bytes()[0..9],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x44, 0x49, 0x53]
    );
    assert!(pack.as_bytes()[9..].iter().all(|&b| b == 0));
}

#[test]
fn lig_places_brightness_at_wire_11() {
    let pack = build_lig(30).expect("30 is valid");
    assert_eq!(
        &pack.as_bytes()[0..12],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x4C, 0x49, 0x47, 0x00, 0x00, 30]
    );
    assert!(pack.as_bytes()[12..].iter().all(|&b| b == 0));
}

#[test]
fn bat_places_len_and_key_at_wire_11_13() {
    // 0x1234 = 4660 bytes, key 7
    let pack = build_bat(0x1234, 7).expect("valid");
    assert_eq!(
        &pack.as_bytes()[0..14],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x42, 0x41, 0x54, 0x00, 0x00, 0x12, 0x34, 7]
    );
    assert!(pack.as_bytes()[14..].iter().all(|&b| b == 0));
}

#[test]
fn stp_is_byte_exact() {
    let pack = build_stp();
    assert_eq!(
        &pack.as_bytes()[0..9],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x53, 0x54, 0x50]
    );
}

#[test]
fn cle_all_uses_ff_at_wire_12() {
    let pack = build_cle_all();
    assert_eq!(
        &pack.as_bytes()[0..13],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x43, 0x4C, 0x45, 0x00, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn cle_key_places_firmware_key_at_wire_12() {
    let pack = build_cle_key(5).expect("key 5 valid");
    assert_eq!(pack.as_bytes()[12], 5);
    assert_eq!(&pack.as_bytes()[6..9], b"CLE");
}

#[test]
fn ver_is_byte_exact() {
    let pack = build_ver();
    assert_eq!(
        &pack.as_bytes()[0..9],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x56, 0x45, 0x52]
    );
}

#[test]
fn bare_han_has_no_crt_prefix() {
    let pack = build_han_bare();
    assert_eq!(&pack.as_bytes()[0..4], &[0x00, 0x48, 0x41, 0x4E]);
    assert!(pack.as_bytes()[4..].iter().all(|&b| b == 0));
}

#[test]
fn sleep_is_crt_plus_han() {
    let pack = build_sleep();
    assert_eq!(
        &pack.as_bytes()[0..9],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x48, 0x41, 0x4E]
    );
}

#[test]
fn mod_places_ascii_mode_at_wire_11() {
    let pack = build_mod(2).expect("mode 2 valid");
    assert_eq!(pack.as_bytes()[11], b'2');
    assert_eq!(&pack.as_bytes()[6..9], b"MOD");
}

#[test]
fn chunk_pack_carries_report_id_and_payload() {
    let chunk = [0xAAu8; 10];
    let pack = ss550_protocol::build_image_chunk(&chunk).expect("chunk valid");
    assert_eq!(pack.as_bytes()[0], 0x00);
    assert_eq!(&pack.as_bytes()[1..11], &chunk);
    assert!(pack.as_bytes()[11..].iter().all(|&b| b == 0));
}

#[test]
fn constants_match_transport() {
    assert_eq!(REPORT_SIZE, 1025);
    assert_eq!(CHUNK_SIZE, 1024);
}

#[test]
fn builders_reject_out_of_range() {
    assert!(build_lig(101).is_err());
    assert!(build_bat(100, 0).is_err(), "firmware keys are 1-based");
    assert!(build_bat(100, 16).is_err(), "max firmware key is 15");
    assert!(build_cle_key(0).is_err());
    assert!(build_mod(10).is_err(), "mode must be 0..9");
    assert!(ss550_protocol::build_image_chunk(&vec![0u8; CHUNK_SIZE + 1]).is_err());
}

#[test]
fn bat_rejects_size_above_be16() {
    assert!(build_bat(65536, 1).is_err());
    assert!(build_bat(65535, 1).is_ok());
}

#[test]
fn connect_is_crt_plus_connect_tag() {
    let pack = build_connect();
    assert_eq!(
        &pack.as_bytes()[0..13],
        &[0x00, 0x43, 0x52, 0x54, 0x00, 0x00, 0x43, 0x4F, 0x4E, 0x4E, 0x45, 0x43, 0x54]
    );
    assert!(pack.as_bytes()[13..].iter().all(|&b| b == 0));
}

#[test]
fn image_chunks_split_and_pad() {
    // Empty → no packs (never sends a bare chunk).
    assert!(build_image_chunks(&[]).is_empty());
    // Single small chunk → one pack.
    let one = build_image_chunks(&[0x11u8; 100]);
    assert_eq!(one.len(), 1);
    // 2500 bytes → ceil(2500/1024) = 3 packs.
    let data = vec![0xABu8; 2500];
    let packs = build_image_chunks(&data);
    assert_eq!(packs.len(), 3);
    // Last pack must carry 2500-2048 = 452 bytes, rest zero.
    let last = &packs[2];
    assert_eq!(&last.as_bytes()[1..1 + 452], &vec![0xABu8; 452]);
    assert!(last.as_bytes()[1 + 452..].iter().all(|&b| b == 0));
    // Exact multiple → no extra empty pack.
    let exact = build_image_chunks(&vec![0u8; 2048]);
    assert_eq!(exact.len(), 2);
}
