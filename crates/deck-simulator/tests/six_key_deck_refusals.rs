//! A refusal leaves nothing behind.
//!
//! Two things are pinned here, and the suite only pins the first of them:
//!
//! - the **order** of the checks: the key is refused before the payload is
//!   looked at, and the payload is refused before any image is taken in. The
//!   case in the suite proves the key-versus-payload order with an empty
//!   payload; this file proves it with the payload that is also wrong, which is
//!   the case a provider that checks in the other order would answer with
//!   `ImageTooLarge`;
//! - **the record is untouched** by a refusal. A deck that half-wrote a paint
//!   before rejecting it is a deck a caller cannot trust to retry.

use deck_api::{Deck, DeckError, KeyId};
use deck_simulator::SixKeyDeck;

/// Every key's paint, in key order, as the deck would show it.
fn record(deck: &SixKeyDeck) -> Vec<Option<Vec<u8>>> {
    (0..deck.geometry().key_count())
        .map(|index| deck.painted(KeyId::new(index)).map(<[u8]>::to_vec))
        .collect()
}

#[test]
fn a_refused_key_is_refused_before_the_payload_is_looked_at() {
    let mut deck = SixKeyDeck::new();
    let over_the_limit = vec![0_u8; deck.image_spec().max_bytes() + 1];
    let key = KeyId::new(6);

    let error = deck
        .set_key_image(key, &over_the_limit)
        .expect_err("key 6 does not exist on a six-key deck");

    // Not `ImageTooLarge`: the cheap question is answered first, and on hardware
    // that is the difference between a refusal and a write.
    assert_eq!(error, DeckError::KeyOutOfRange { key, key_count: 6 });
}

#[test]
fn a_refused_key_leaves_the_paint_record_alone() {
    let mut deck = SixKeyDeck::new();
    deck.set_key_image(KeyId::new(0), &[0xFF, 0xD8, 0xFF])
        .expect("a three-byte payload is under the limit");
    let before = record(&deck);

    let error = deck
        .set_key_image(KeyId::new(6), &[])
        .expect_err("key 6 does not exist");

    assert_eq!(
        error,
        DeckError::KeyOutOfRange {
            key: KeyId::new(6),
            key_count: 6
        }
    );
    assert_eq!(record(&deck), before);
}

#[test]
fn an_oversized_payload_leaves_the_paint_record_alone() {
    let mut deck = SixKeyDeck::new();
    deck.set_key_image(KeyId::new(0), &[0xFF, 0xD8, 0xFF])
        .expect("a three-byte payload is under the limit");
    let before = record(&deck);

    let max_bytes = deck.image_spec().max_bytes();
    let payload = vec![0_u8; max_bytes + 1];
    let error = deck
        .set_key_image(KeyId::new(1), &payload)
        .expect_err("one byte over the limit is over the limit");

    assert_eq!(
        error,
        DeckError::ImageTooLarge {
            bytes: max_bytes + 1,
            max_bytes,
        }
    );
    assert_eq!(record(&deck), before);
}

#[test]
fn an_accepted_paint_lands_on_the_key_it_names_and_on_no_other() {
    let mut deck = SixKeyDeck::new();

    deck.set_key_image(KeyId::new(4), &[1, 2, 3, 4])
        .expect("key 4 exists and the payload is under the limit");

    assert_eq!(deck.painted(KeyId::new(4)), Some(&[1_u8, 2, 3, 4][..]));
    assert_eq!(deck.painted_keys(), vec![KeyId::new(4)]);
    for index in [0, 1, 2, 3, 5] {
        assert_eq!(
            deck.painted(KeyId::new(index)),
            None,
            "key {index} was painted by a call that never named it"
        );
    }
}

#[test]
fn clearing_one_key_forgets_only_that_key() {
    let mut deck = SixKeyDeck::new();
    deck.set_key_image(KeyId::new(0), &[1])
        .expect("key 0 exists");
    deck.set_key_image(KeyId::new(5), &[2])
        .expect("key 5 exists");

    deck.clear_key(KeyId::new(0)).expect("key 0 exists");

    assert_eq!(deck.painted(KeyId::new(0)), None);
    assert_eq!(deck.painted(KeyId::new(5)), Some(&[2_u8][..]));

    let error = deck
        .clear_key(KeyId::new(6))
        .expect_err("key 6 does not exist");
    assert_eq!(
        error,
        DeckError::KeyOutOfRange {
            key: KeyId::new(6),
            key_count: 6
        }
    );
}

#[test]
fn clearing_every_key_empties_the_whole_record() {
    let mut deck = SixKeyDeck::new();
    for index in 0..6 {
        deck.set_key_image(KeyId::new(index), &[index as u8])
            .expect("every key exists");
    }

    deck.clear_all().expect("this deck clears every key");

    assert!(deck.painted_keys().is_empty());
    assert!(record(&deck).iter().all(Option::is_none));
}
