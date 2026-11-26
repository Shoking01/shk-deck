//! A successful preflight must reach the device and leave the deck **awake**.
//!
//! Measured on 2026-11-25: the window connected, preflighted, and its own log
//! said `heartbeat not writing: Asleep` one second later. Two things were wrong
//! and both are pinned here:
//!
//! * `DeckHandle` did not forward `preflight` at all, so the trait's default
//!   answered `Ok` without writing anything and the window's preflight never
//!   reached the device;
//! * `Device::preflight` wrote the same `CRT|DIS` that `Device::initialize`
//!   sends and then left `initialized` false, so `keepalive` declined forever.
//!
//! Either one alone leaves the deck hearing silence, resetting itself, and
//! killing the handle with `0x48F` every few minutes — the reconnect loop the
//! runbook calls a dead handle.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use deck_api::{Deck, DeckHandle, Transport, TransportError};
use ss550_driver::Ss550Deck;

/// Longer than the driver's 2 s heartbeat throttle, so a write is genuinely due.
const PAST_THE_THROTTLE: Duration = Duration::from_millis(2100);

#[derive(Default)]
struct Wire {
    written: Vec<Vec<u8>>,
    pending: VecDeque<Vec<u8>>,
}

#[derive(Clone, Default)]
struct FakeTransport {
    wire: Arc<Mutex<Wire>>,
}

impl FakeTransport {
    fn new() -> Self {
        Self::default()
    }

    fn written(&self) -> usize {
        self.wire
            .lock()
            .expect("the fake transport's wire is never poisoned")
            .written
            .len()
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
            None => Ok(0),
        }
    }

    fn read_control(&mut self, _buf: &mut [u8]) -> Result<usize, TransportError> {
        Ok(0)
    }
}

/// The deck behind the seam, over a fake wire, exactly as the window builds it.
fn deck() -> (DeckHandle, FakeTransport) {
    let transport = FakeTransport::new();
    let deck = DeckHandle::new(Box::new(Ss550Deck::over_transport(Box::new(
        transport.clone(),
    ))));
    (deck, transport)
}

#[test]
fn a_successful_preflight_reaches_the_device() {
    // Through the handle, because that is the path the window takes. A handle
    // that forgets to forward `preflight` answers `Ok` having written nothing.
    let (mut deck, transport) = deck();

    deck.preflight()
        .expect("the fake transport accepts every report");

    assert_eq!(
        transport.written(),
        1,
        "the preflight never reached the device: the handle answered from the \
         trait's default instead of forwarding"
    );
}

#[test]
fn a_successful_preflight_leaves_the_heartbeat_writing() {
    let (mut deck, transport) = deck();

    deck.preflight()
        .expect("the fake transport accepts every report");

    // The preflight just wrote the wake. From here the heartbeat has a deck to
    // keep awake, so once the throttle has passed it must write. A driver that
    // still believes the deck is asleep sends nothing, the deck hears silence,
    // and it resets itself out from under a handle that never wrote a byte.
    std::thread::sleep(PAST_THE_THROTTLE);
    deck.keep_alive()
        .expect("the fake transport accepts every report");

    assert_eq!(
        transport.written(),
        2,
        "the preflight woke the deck but left the driver thinking it is asleep, \
         so the heartbeat never wrote"
    );
}

#[test]
fn a_deck_that_was_never_woken_stays_asleep() {
    // The heartbeat is not a wake: a sleeping deck is supposed to be idle, and
    // a `DIS` sent to one that was never initialized would defeat `sleep`.
    let (mut deck, transport) = deck();

    std::thread::sleep(PAST_THE_THROTTLE);
    deck.keep_alive()
        .expect("a declined heartbeat is not an error");

    assert_eq!(
        transport.written(),
        0,
        "a deck that was never woken must not receive a heartbeat"
    );
}
