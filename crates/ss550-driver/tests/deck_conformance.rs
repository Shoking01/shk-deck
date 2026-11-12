//! The SS550 as a `Deck`, over a transport that keeps the bytes.
//!
//! Everything here runs on a fake transport: no device is opened, no byte
//! reaches a desk. That is the point of the seam — the deck's semantics can be
//! checked without hardware, and the hardware test in `tests/hardware.rs` is
//! the only file that opens anything.
//!
//! Three rules are pinned here, and each one is a rule the shared suite cannot
//! state for us:
//!
//! - the deck passes `deck_conformance::run` without a single `Fail`, **and
//!   the run writes nothing**, because the shared suite only makes calls it
//!   expects to be refused;
//! - the key is checked before the image: an out-of-range key is reported even
//!   when the payload is also over the limit;
//! - a command the registry forbids is refused **before** a byte leaves, so the
//!   fake transport records nothing for `clear_key`, `sleep` and `identity`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deck_api::{
    ButtonState, Deck, DeckError, DeckOperation, KeyEvent, KeyId, Transport, TransportError,
};
use deck_conformance::Outcome;
use ss550_driver::Ss550Deck;
use ss550_protocol::{CHUNK_SIZE, INPUT_REPORT_SIZE};

/// What the fake transport has seen, shared with the test that owns it.
#[derive(Default)]
struct Wire {
    /// One entry per accepted write, in order.
    written: Vec<Vec<u8>>,
    /// Reports waiting to be read, oldest first.
    pending: VecDeque<Vec<u8>>,
}

/// A transport that records writes and replays the reports a test queues.
///
/// Cloning it hands out another handle to the **same** wire, so the test can
/// inspect the bytes after the deck has taken ownership of its own copy.
#[derive(Clone, Default)]
struct FakeTransport {
    wire: Arc<Mutex<Wire>>,
}

impl FakeTransport {
    fn new() -> Self {
        Self::default()
    }

    /// Every report the deck handed to this transport.
    fn written(&self) -> Vec<Vec<u8>> {
        self.wire
            .lock()
            .expect("the fake transport's wire is never poisoned")
            .written
            .clone()
    }

    /// Queues one input report for the next read.
    fn queue(&self, report: Vec<u8>) {
        self.wire
            .lock()
            .expect("the fake transport's wire is never poisoned")
            .pending
            .push_back(report);
    }
}

impl Transport for FakeTransport {
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        let mut wire = self
            .wire
            .lock()
            .expect("the fake transport's wire is never poisoned");
        wire.written.push(bytes.to_vec());
        Ok(bytes.len())
    }

    fn read_input(&mut self, buf: &mut [u8], _timeout: Duration) -> Result<usize, TransportError> {
        let mut wire = self
            .wire
            .lock()
            .expect("the fake transport's wire is never poisoned");
        match wire.pending.pop_front() {
            Some(report) => {
                let read = report.len().min(buf.len());
                buf[..read].copy_from_slice(&report[..read]);
                Ok(read)
            }
            // Zero is "nothing waiting", which is what a device with no press
            // reports — not an error.
            None => Ok(0),
        }
    }

    fn read_control(&mut self, _buf: &mut [u8]) -> Result<usize, TransportError> {
        Ok(0)
    }
}

/// A key-edge input report, as the deck sends one.
///
/// `read_key` uses the **read** numbering (1 is the top-left); the driver flips
/// it back to the paint numbering before it reports a key.
fn key_report(read_key: u8, pressed: bool) -> Vec<u8> {
    let mut report = vec![0u8; INPUT_REPORT_SIZE];
    report[0..3].copy_from_slice(b"ACK");
    report[5..7].copy_from_slice(b"OK");
    report[9] = read_key;
    report[10] = u8::from(pressed);
    report
}

/// A deck over a fake transport, plus the fake the test keeps.
fn deck() -> (Ss550Deck, FakeTransport) {
    let transport = FakeTransport::new();
    let deck = Ss550Deck::over_transport(Box::new(transport.clone()));
    (deck, transport)
}

/// The three-byte tag a `CRT` report carries at wire `[6..9]`.
fn tag(report: &[u8]) -> &[u8] {
    &report[6..9]
}

/// A solid-colour JPEG of the given size, as a caller would hand one over.
fn solid_jpeg(width: u16, height: u16) -> Vec<u8> {
    let rgb = vec![0x20_u8; usize::from(width) * usize::from(height) * 3];
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 75)
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .expect("a synthetic image encodes");
    jpeg
}

#[test]
fn the_ss550_deck_passes_the_conformance_suite() {
    let (mut deck, transport) = deck();

    let results = deck_conformance::run(&mut deck);

    assert!(!results.is_empty(), "the suite ran no cases at all");
    let failures: Vec<String> = results
        .iter()
        .filter(|case| case.outcome == Outcome::Fail)
        .map(|case| format!("{}: {}", case.name, case.detail))
        .collect();
    assert!(
        failures.is_empty(),
        "the SS550 deck fails the shared suite:\n{}",
        failures.join("\n")
    );

    // The suite only makes calls it expects to be refused, so a deck that
    // passes it has written nothing at all. If this fails, some case reached
    // the wire, and the suite is no longer safe to point at a deck on a desk.
    assert!(
        transport.written().is_empty(),
        "the conformance suite wrote {} report(s); it must only make calls it expects to be refused",
        transport.written().len()
    );
}

#[test]
fn an_out_of_range_key_is_refused_before_the_image_is_inspected() {
    let (mut deck, transport) = deck();

    let key_count = deck.geometry().key_count();
    let key = KeyId::new(key_count);
    // Both refusals are available; the key is the cheaper question and the one
    // the caller has to fix first, so it is the one reported.
    let over_the_limit = vec![0_u8; deck.image_spec().max_bytes() + 1];

    let error = deck
        .set_key_image(key, &over_the_limit)
        .expect_err("a key past the end must be refused");

    assert_eq!(
        error,
        DeckError::KeyOutOfRange { key, key_count },
        "the key must be reported, not the image"
    );
    assert!(
        transport.written().is_empty(),
        "a refused image must not reach the wire"
    );
}

#[test]
fn a_valid_image_is_painted_and_the_bytes_are_forwarded_unchanged() {
    let (mut deck, transport) = deck();
    let spec = deck.image_spec();
    let jpeg = solid_jpeg(spec.width(), spec.height());
    assert!(
        jpeg.len() <= spec.max_bytes(),
        "the test image must fit the deck's own limit"
    );

    deck.set_key_image(KeyId::FIRST, &jpeg)
        .expect("a JPEG of the declared size, under the limit, is paintable");

    let reports = transport.written();
    let chunk_count = jpeg.len().div_ceil(CHUNK_SIZE);
    assert_eq!(
        reports.len(),
        3 + chunk_count,
        "one DIS to wake, one BAT, the JPEG chunks, one STP to commit"
    );
    assert_eq!(tag(&reports[0]), b"DIS");
    assert_eq!(tag(&reports[1]), b"BAT");
    assert_eq!(
        u16::from_be_bytes([reports[1][11], reports[1][12]]) as usize,
        jpeg.len(),
        "BAT must announce the payload the caller handed over"
    );
    assert_eq!(reports[1][13], 1, "key 0 paints as firmware key 1");
    assert_eq!(tag(&reports[reports.len() - 1]), b"STP");

    // The caller's bytes must be forwarded as they are: no decode, no
    // re-encode, no truncation.
    let mut forwarded = Vec::new();
    for chunk in &reports[2..reports.len() - 1] {
        forwarded.extend_from_slice(&chunk[1..]);
    }
    assert_eq!(&forwarded[..jpeg.len()], &jpeg[..]);
}

#[test]
fn clear_all_reaches_the_wire_after_the_wake() {
    let (mut deck, transport) = deck();

    deck.clear_all()
        .expect("clear-all is verified and advertised by this deck");

    let reports = transport.written();
    assert_eq!(reports.len(), 3, "one DIS to wake, one CLE, one STP");
    assert_eq!(tag(&reports[0]), b"DIS");
    assert_eq!(tag(&reports[1]), b"CLE");
    assert_eq!(reports[1][12], 0xFF, "0xFF selects every key");
    assert_eq!(tag(&reports[2]), b"STP");
}

#[test]
fn a_refused_command_never_reaches_the_wire() {
    let (mut deck, transport) = deck();

    // All three are `unverified` in the protocol registry, so `ensure_verified`
    // refuses them before a byte leaves. The seam says the same thing with its
    // own typed error instead of a silent success.
    assert_eq!(
        deck.clear_key(KeyId::FIRST),
        Err(DeckError::Unsupported {
            operation: DeckOperation::ClearKey
        })
    );
    assert_eq!(
        deck.sleep(),
        Err(DeckError::Unsupported {
            operation: DeckOperation::Sleep
        })
    );
    assert_eq!(
        deck.identity(),
        Err(DeckError::Unsupported {
            operation: DeckOperation::Identity
        })
    );

    // This deck has no dials, and says so with the count.
    assert_eq!(
        deck.set_dial(0, 0),
        Err(DeckError::DialOutOfRange {
            dial: 0,
            dial_count: 0
        })
    );

    assert!(
        transport.written().is_empty(),
        "a refused command wrote {} report(s) on its way to being refused",
        transport.written().len()
    );
}

#[test]
fn a_brightness_past_the_range_is_refused_before_the_clamp() {
    let (mut deck, transport) = deck();

    let value = deck_api::VALUE_MAX + 1;
    assert_eq!(
        deck.set_brightness(value),
        Err(DeckError::value_out_of_range(
            DeckOperation::Brightness,
            value
        )),
        "the seam refuses an out-of-range value instead of clamping it"
    );
    assert!(
        transport.written().is_empty(),
        "a refused brightness must not reach the wire"
    );
}

#[test]
fn a_poll_maps_the_deck_report_to_a_seam_event() {
    let (mut deck, transport) = deck();
    // Read key 1 is the top-left; it paints as firmware key 11, which is
    // OpenDeck key 10.
    transport.queue(key_report(1, true));

    let events = deck
        .poll_events(Duration::ZERO)
        .expect("a read on a working transport is not an error");

    assert_eq!(
        events,
        vec![KeyEvent::new(KeyId::new(10), ButtonState::Pressed)],
        "the driver's key numbering must be carried through unchanged"
    );
}

#[test]
fn a_poll_with_nothing_waiting_returns_at_once_and_empty() {
    let (mut deck, _transport) = deck();

    let started = Instant::now();
    let events = deck
        .poll_events(Duration::ZERO)
        .expect("a read on a working transport is not an error");

    assert!(
        events.is_empty(),
        "nothing was pressed, so nothing is reported"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a zero timeout must not block the caller"
    );
}
