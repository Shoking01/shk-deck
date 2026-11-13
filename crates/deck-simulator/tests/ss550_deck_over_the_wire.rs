//! The real driver, over the emulated wire.
//!
//! The other tests in this crate prove each half in isolation: `wire_fake.rs`
//! hands the fake packs built by hand, and `six_key_deck_conformance.rs` runs
//! the suite against a deck with no transport at all. This file is the join:
//! `Ss550Deck` — the same type the application drives — over `Ss550WireFake`,
//! so the driver's real bytes are decoded by a transport that is not `hidapi`.
//!
//! That is what makes the fake worth having. If it only ever saw packs a test
//! wrote, it would prove that the test and the fake agree; with the driver on
//! top, it proves the driver's upload, its wake, its clear and its input
//! decoding are the ones the wire expects, without a device on the desk.
//!
//! Nothing here touches hardware and nothing here writes a file: the fake keeps
//! its transcript in memory.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use deck_api::{
    ButtonState, Deck, DeckError, DeckOperation, KeyEvent, KeyId, Transport, TransportError,
};
use deck_simulator::{Ss550Command, Ss550WireFake};
use ss550_driver::Ss550Deck;
use ss550_protocol::INPUT_REPORT_SIZE;

/// The fake behind a lock, so the deck can own its transport and the test can
/// still read what the deck put on the wire.
///
/// Cloning hands out another handle to the **same** fake, which is the only way
/// to inspect a transport after `Ss550Deck` has taken ownership of it.
#[derive(Clone)]
struct SharedWire {
    inner: Arc<Mutex<Ss550WireFake>>,
}

impl Transport for SharedWire {
    fn write_report(&mut self, bytes: &[u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .write_report(bytes)
    }

    fn read_input(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_input(buf, timeout)
    }

    fn read_control(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        self.inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .read_control(buf)
    }
}

/// A deck on the emulated wire, plus the handle a test needs to read it.
struct Bench {
    deck: Ss550Deck,
    wire: SharedWire,
}

impl Bench {
    fn new() -> Self {
        let fake = Ss550WireFake::new();
        let wire = SharedWire {
            inner: Arc::new(Mutex::new(fake)),
        };
        let deck = Ss550Deck::over_transport(Box::new(wire.clone()));
        Self { deck, wire }
    }

    /// Every command the fake understood, in arrival order.
    fn transcript(&self) -> Vec<Ss550Command> {
        self.wire
            .inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .transcript()
            .to_vec()
    }

    /// Queues one input report for the deck's next poll.
    fn queue_input(&self, report: &[u8]) {
        self.wire
            .inner
            .lock()
            .expect("the fake's wire is never poisoned")
            .queue_input_report(report);
    }
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

#[test]
fn the_driver_paints_a_key_over_the_emulated_wire() {
    let mut bench = Bench::new();
    let spec = bench.deck.image_spec();
    let jpeg = solid_jpeg(spec.width(), spec.height());
    assert!(
        jpeg.len() <= spec.max_bytes(),
        "the test image must fit the deck's own limit"
    );

    bench
        .deck
        .set_key_image(KeyId::FIRST, &jpeg)
        .expect("a JPEG of the deck's own size is paintable");

    // One entry per command the fake decoded: the wake, then the transfer at
    // its flush. The image is one entry however many chunk packs carried it.
    assert_eq!(
        bench.transcript(),
        [
            Ss550Command::DisplayOn,
            Ss550Command::ImageTransfer {
                firmware_key: 1,
                bytes: jpeg.len(),
            },
        ],
        "the driver must wake the deck, then upload to firmware key 1"
    );
}

#[test]
fn the_driver_clears_and_sets_brightness_over_the_emulated_wire() {
    let mut bench = Bench::new();

    bench
        .deck
        .set_brightness(40)
        .expect("40 is inside the deck's own range");
    bench
        .deck
        .clear_all()
        .expect("clear-all is a capability of this deck");

    assert_eq!(
        bench.transcript(),
        [
            Ss550Command::DisplayOn,
            Ss550Command::Brightness { value: 40 },
            // The clear reuses the wake: `initialize` is idempotent, so no
            // second DIS is on the wire before the CLE.
            Ss550Command::ClearAll,
        ],
        "the brightness and the clear must reach the wire as themselves"
    );
}

#[test]
fn the_driver_reads_a_press_over_the_emulated_wire() {
    let mut bench = Bench::new();
    bench.queue_input(&key_report(1, true));

    let events = bench
        .deck
        .poll_events(Duration::ZERO)
        .expect("a read on a working wire is not an error");

    assert_eq!(
        events,
        vec![KeyEvent::new(KeyId::new(10), ButtonState::Pressed)],
        "the driver's key numbering must survive the trip through the fake"
    );
}

#[test]
fn a_command_the_registry_forbids_writes_nothing_to_the_emulated_wire() {
    let mut bench = Bench::new();

    assert_eq!(
        bench.deck.sleep(),
        Err(DeckError::Unsupported {
            operation: DeckOperation::Sleep
        })
    );
    assert!(
        bench.transcript().is_empty(),
        "the gate must refuse before the fake sees a pack"
    );
}
