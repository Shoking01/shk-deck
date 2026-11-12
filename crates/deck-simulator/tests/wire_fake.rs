//! The other half of the seam: the bytes, decoded.
//!
//! This is a `Transport`, not a deck, and it is the only place in this crate
//! that knows the wire format. A deck that sits on it never sees a pack, and
//! this test never touches a device: it builds real packs with `ss550-protocol`
//! and reads back what the fake understood.
//!
//! The fake is deliberately strict. It is more useful as a double that says
//! "that was not a command" than as one that silently swallows anything a deck
//! hands it, because a deck writing the wrong bytes is exactly the failure the
//! seam is meant to make visible.

use std::time::Duration;

use deck_api::{Transport, TransportError};
use deck_simulator::{Ss550Command, Ss550WireFake};
use ss550_protocol::{
    build_bat, build_cle_all, build_connect, build_dis, build_image_chunk, build_lig, build_stp,
    CHUNK_SIZE, REPORT_SIZE,
};

/// Writes one pack through the seam and expects the fake to take all of it.
fn send(fake: &mut Ss550WireFake, bytes: &[u8]) {
    let transport: &mut dyn Transport = fake;
    let written = transport
        .write_report(bytes)
        .expect("the fake takes a pack");
    assert_eq!(written, REPORT_SIZE, "a report is one full pack");
}

#[test]
fn the_fake_decodes_brightness_keep_alive_and_an_image_transfer() {
    let mut fake = Ss550WireFake::new();

    send(
        &mut fake,
        build_lig(30).expect("30 is a valid brightness").as_bytes(),
    );
    send(&mut fake, build_connect().as_bytes());

    // A payload that spans two chunks, so the byte count is the sum of them and
    // not the size of whichever chunk arrived last.
    let jpeg = vec![0x5A_u8; CHUNK_SIZE + 476];
    send(
        &mut fake,
        build_bat(jpeg.len(), 7)
            .expect("7 is a firmware key")
            .as_bytes(),
    );
    for chunk in jpeg.chunks(CHUNK_SIZE) {
        send(
            &mut fake,
            build_image_chunk(chunk)
                .expect("a chunk is at most CHUNK_SIZE")
                .as_bytes(),
        );
    }
    send(&mut fake, build_stp().as_bytes());

    assert_eq!(
        fake.command_names(),
        ["brightness", "keep_alive", "image_transfer"],
        "the transcript names the commands in the order they arrived"
    );
    assert_eq!(
        fake.transcript(),
        [
            Ss550Command::Brightness { value: 30 },
            Ss550Command::KeepAlive,
            Ss550Command::ImageTransfer {
                firmware_key: 7,
                bytes: jpeg.len(),
            },
        ]
    );
}

#[test]
fn a_transfer_is_one_entry_however_many_packs_it_takes() {
    let mut fake = Ss550WireFake::new();
    let jpeg = vec![0x01_u8; 3 * CHUNK_SIZE];

    send(
        &mut fake,
        build_bat(jpeg.len(), 1)
            .expect("1 is a firmware key")
            .as_bytes(),
    );
    for chunk in jpeg.chunks(CHUNK_SIZE) {
        send(
            &mut fake,
            build_image_chunk(chunk)
                .expect("a chunk is at most CHUNK_SIZE")
                .as_bytes(),
        );
    }
    send(&mut fake, build_stp().as_bytes());

    assert_eq!(fake.transcript().len(), 1);
    assert_eq!(
        fake.transcript()[0],
        Ss550Command::ImageTransfer {
            firmware_key: 1,
            bytes: 3 * CHUNK_SIZE,
        }
    );
}

#[test]
fn the_flush_after_a_clear_is_not_a_command_of_its_own() {
    let mut fake = Ss550WireFake::new();

    send(&mut fake, build_cle_all().as_bytes());
    send(&mut fake, build_stp().as_bytes());
    send(&mut fake, build_dis().as_bytes());

    assert_eq!(fake.command_names(), ["clear_all", "display_on"]);
}

#[test]
fn every_command_name_comes_from_the_registry() {
    let mut fake = Ss550WireFake::new();

    send(
        &mut fake,
        build_lig(0).expect("0 is a valid brightness").as_bytes(),
    );
    send(&mut fake, build_connect().as_bytes());
    send(&mut fake, build_cle_all().as_bytes());
    send(&mut fake, build_stp().as_bytes());
    send(&mut fake, build_dis().as_bytes());

    for entry in fake.transcript() {
        assert_eq!(
            entry.name(),
            entry.command().name(),
            "a transcript name that is not the registry's name is a second vocabulary"
        );
    }
}

#[test]
fn a_failed_write_is_reported_and_never_reaches_the_transcript() {
    // `TransportError` is deliberately not `Clone`, so the expectation is built
    // twice rather than copied: the seam's errors are values, not handles.
    let stalled = || TransportError::Stalled("the device stopped answering".to_owned());

    let mut fake = Ss550WireFake::new();
    fake.fail_next_write(stalled());

    let lig = build_lig(10).expect("10 is a valid brightness");
    let error = fake
        .write_report(lig.as_bytes())
        .expect_err("the next write was told to fail");

    assert_eq!(error, stalled());
    assert!(
        fake.transcript().is_empty(),
        "a pack that never left the host was not sent"
    );

    // One instruction, one failure: the write after it is a normal write.
    send(&mut fake, lig.as_bytes());
    assert_eq!(fake.command_names(), ["brightness"]);
}

#[test]
fn a_report_that_is_not_a_pack_is_refused() {
    let mut fake = Ss550WireFake::new();

    let short = fake
        .write_report(&[0_u8; 64])
        .expect_err("a report is one full pack");
    assert!(matches!(short, TransportError::Io(_)), "got {short}");

    let nonsense = vec![0xAA_u8; REPORT_SIZE];
    let unknown = fake
        .write_report(&nonsense)
        .expect_err("those bytes are no command this deck speaks");
    assert!(matches!(unknown, TransportError::Io(_)), "got {unknown}");

    assert!(fake.transcript().is_empty());
}

#[test]
fn reads_answer_with_nothing_rather_than_waiting() {
    let mut fake = Ss550WireFake::new();
    let mut buffer = [0_u8; 512];

    let read = fake
        .read_input(&mut buffer, Duration::from_secs(60))
        .expect("an empty read is not a failure");
    assert_eq!(read, 0);
    assert_eq!(
        fake.read_control(&mut buffer).expect("no control report"),
        0
    );
}

#[test]
fn a_queued_input_report_arrives_on_the_next_read() {
    let mut fake = Ss550WireFake::new();
    fake.queue_input_report(&[
        0x41, 0x43, 0x4B, 0x00, 0x00, 0x4F, 0x4B, 0x00, 0x00, 0x01, 0x01,
    ]);

    let mut buffer = [0_u8; 512];
    let read = fake
        .read_input(&mut buffer, Duration::ZERO)
        .expect("a queued report is a successful read");

    assert_eq!(read, 11);
    assert_eq!(
        &buffer[..11],
        &[0x41, 0x43, 0x4B, 0x00, 0x00, 0x4F, 0x4B, 0x00, 0x00, 0x01, 0x01]
    );
    assert_eq!(
        fake.read_input(&mut buffer, Duration::ZERO)
            .expect("the queue is empty now"),
        0
    );
}

#[test]
fn a_stalled_read_is_reported_as_a_stall() {
    let stalled = || TransportError::Stalled("the cable is out".to_owned());

    let mut fake = Ss550WireFake::new();
    fake.fail_next_read(stalled());

    let mut buffer = [0_u8; 512];
    let error = fake
        .read_input(&mut buffer, Duration::ZERO)
        .expect_err("the next read was told to stall");

    assert_eq!(error, stalled());
}
