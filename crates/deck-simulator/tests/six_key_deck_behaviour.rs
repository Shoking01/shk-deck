//! What the deck does when it is asked, not just what it declares.
//!
//! The conformance suite never paints a key and never expects a success, so it
//! cannot show that this deck accepts anything at all. These tests are the other
//! half: the operations the deck advertises have to work, and the ones it does
//! not advertise have to refuse with the operation named.

use std::time::{Duration, Instant};

use deck_api::{ButtonState, Deck, DeckError, DeckOperation, KeyEvent, KeyId, VALUE_MAX};
use deck_simulator::SixKeyDeck;

#[test]
fn brightness_is_refused_as_unsupported() {
    let mut deck = SixKeyDeck::new();

    let error = deck
        .set_brightness(50)
        .expect_err("this deck has no brightness to set");

    assert_eq!(
        error,
        DeckError::Unsupported {
            operation: DeckOperation::Brightness,
        }
    );
}

#[test]
fn sleep_is_refused_as_unsupported() {
    let mut deck = SixKeyDeck::new();

    let error = deck.sleep().expect_err("this deck has no sleep command");

    assert_eq!(
        error,
        DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        }
    );
}

#[test]
fn both_dials_take_a_position_and_nothing_lands_on_a_dial_that_is_not_there() {
    let mut deck = SixKeyDeck::new();

    deck.set_dial(0, 0).expect("dial 0 exists");
    deck.set_dial(1, VALUE_MAX).expect("dial 1 exists");

    assert_eq!(deck.dial_position(0), Some(0));
    assert_eq!(deck.dial_position(1), Some(VALUE_MAX));

    let error = deck
        .set_dial(2, 0)
        .expect_err("this deck has two dials, numbered 0 and 1");
    assert_eq!(
        error,
        DeckError::DialOutOfRange {
            dial: 2,
            dial_count: 2,
        }
    );
    assert_eq!(
        deck.dial_position(2),
        None,
        "a refused dial must not invent a position"
    );
}

#[test]
fn a_dial_position_past_the_range_is_refused_rather_than_clamped() {
    let mut deck = SixKeyDeck::new();

    let error = deck
        .set_dial(0, VALUE_MAX + 1)
        .expect_err("101 is past the top of the range");

    assert_eq!(
        error,
        DeckError::value_out_of_range(DeckOperation::SetDial, VALUE_MAX + 1)
    );
    assert_eq!(
        deck.dial_position(0),
        None,
        "a refused value must not move the dial"
    );
}

#[test]
fn poll_events_answers_promptly_with_nothing_waiting() {
    let mut deck = SixKeyDeck::new();

    let started = Instant::now();
    let events = deck
        .poll_events(Duration::from_secs(30))
        .expect("an empty poll is not a failure");
    let waited = started.elapsed();

    assert!(events.is_empty());
    assert!(
        waited < Duration::from_secs(1),
        "the simulator waited {waited:?} for an event nobody queued"
    );
}

#[test]
fn poll_events_returns_the_presses_a_test_queued_in_order() {
    let mut deck = SixKeyDeck::new();

    let press = KeyEvent::new(KeyId::new(3), ButtonState::Pressed);
    let release = KeyEvent::new(KeyId::new(3), ButtonState::Released);
    deck.queue_event(press);
    deck.queue_event(release);

    let events = deck.poll_events(Duration::ZERO).expect("a poll succeeds");

    assert_eq!(events, vec![press, release]);
    assert!(
        deck.poll_events(Duration::ZERO)
            .expect("a second poll succeeds")
            .is_empty(),
        "a poll hands over the queue, it does not repeat it"
    );
}

#[test]
fn reconnect_succeeds() {
    let mut deck = SixKeyDeck::new();

    deck.reconnect()
        .expect("there is no cable to pull on a simulator");
}
