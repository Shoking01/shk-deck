#![cfg(feature = "hardware-tests")]

//! Hardware-in-the-loop checks for the SS550 deck.
//!
//! **These tests need a real deck plugged in, and not the daily driver.** The
//! only commands this milestone adds to the wire are the ones already verified,
//! but the point of a hardware run is that it is the first thing that sees a
//! real device, and a wrong report halts the USB endpoint until the deck is
//! power cycled for 60 s (`docs/troubleshooting/deck-stuck.md`). Use a
//! dedicated test unit, as `AGENTS.md` §4 requires.
//!
//! The shared conformance suite only makes calls it expects to be refused, so
//! it is safe to point at a working deck; the key-press case is the one that
//! needs a human.
//!
//! Run them with:
//!
//! ```text
//! cargo test -p ss550-driver --features hardware-tests -- --ignored --nocapture
//! ```
//!
//! Both tests are `#[ignore]`d and the whole file sits behind the
//! `hardware-tests` feature, so a plain `cargo test` never opens a device.

use std::time::{Duration, Instant};

use deck_api::Deck;
use deck_conformance::Outcome;
use ss550_driver::{Ss550Deck, LONG_CONNECT_TIMEOUT};

/// How long the key-press case waits for a human to touch a key.
const PRESS_WINDOW: Duration = Duration::from_secs(15);

/// How long one poll waits inside that window.
const POLL_SLICE: Duration = Duration::from_millis(250);

/// Connects to the deck, giving its boot the whole long timeout.
fn connect() -> Ss550Deck {
    Ss550Deck::connect_with_timeout(LONG_CONNECT_TIMEOUT)
        .expect("a deck must be attached to run the hardware tests")
}

#[test]
#[ignore = "needs a real SS550 deck on a dedicated test unit"]
fn the_deck_on_the_desk_passes_the_conformance_suite() {
    let mut deck = connect();

    let results = deck_conformance::run(&mut deck);

    for case in &results {
        println!("{:12} {} {}", case.outcome, case.name, case.detail);
    }

    assert!(!results.is_empty(), "the suite ran no cases at all");
    let failures: Vec<String> = results
        .iter()
        .filter(|case| case.outcome == Outcome::Fail)
        .map(|case| format!("{}: {}", case.name, case.detail))
        .collect();
    assert!(
        failures.is_empty(),
        "the deck on the desk fails the shared suite:\n{}",
        failures.join("\n")
    );
}

/// The hardware-only case, deliberately outside the shared suite: a key press
/// cannot be checked without a human, and a suite that needs one is not a
/// suite CI can run.
#[test]
#[ignore = "needs a real SS550 deck and a human to press a key"]
fn a_key_press_is_reported_through_the_seam() {
    let mut deck = connect();

    println!(
        "press any key on the deck within {} s",
        PRESS_WINDOW.as_secs()
    );

    let deadline = Instant::now() + PRESS_WINDOW;
    let mut pressed = false;
    while Instant::now() < deadline && !pressed {
        let events = deck
            .poll_events(POLL_SLICE)
            .expect("polling a working deck is not an error");
        for event in events {
            println!("{:?} on key {}", event.state(), event.key().index());
            pressed = true;
        }
    }

    assert!(
        pressed,
        "no key press arrived in {} s: run this with a human at the deck",
        PRESS_WINDOW.as_secs()
    );
}
