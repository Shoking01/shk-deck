//! The second deck's shape, asserted through the seam.
//!
//! The numbers below are the point of the task: a seam proved by a copy of the
//! first implementation proves nothing, so the second deck differs in key count,
//! geometry, image size and capabilities. Every assertion goes through
//! `&mut dyn Deck`, so nothing here can be satisfied by reading a private field.

use deck_api::{Deck, ImageFormat, KeyId};
use deck_simulator::SixKeyDeck;

/// The deck as the code above the seam sees it: one `Deck`, no concrete type.
fn as_seam(deck: &mut SixKeyDeck) -> &mut dyn Deck {
    deck
}

#[test]
fn six_keys_in_two_rows_of_three() {
    let mut deck = SixKeyDeck::new();
    let geometry = as_seam(&mut deck).geometry();

    assert_eq!(geometry.key_count(), 6);
    assert_eq!(geometry.rows(), 2);
    assert_eq!(geometry.columns(), 3);

    // The last key exists and lands in the last cell; one past it does not.
    assert_eq!(geometry.position(KeyId::new(5)), Some((1, 2)));
    assert!(geometry.contains(KeyId::new(5)));
    assert_eq!(geometry.position(KeyId::new(6)), None);
    assert!(!geometry.contains(KeyId::new(6)));
}

#[test]
fn seventy_two_pixel_images_with_a_payload_limit() {
    let mut deck = SixKeyDeck::new();
    let spec = as_seam(&mut deck).image_spec();

    assert_eq!(spec.width(), 72);
    assert_eq!(spec.height(), 72);
    assert_eq!(spec.format(), ImageFormat::Jpeg);
    assert!(
        spec.max_bytes() > 0,
        "a zero-byte limit is an image nobody can paint"
    );
}

#[test]
fn two_dials_no_brightness_and_no_sleep() {
    let mut deck = SixKeyDeck::new();
    let capabilities = as_seam(&mut deck).capabilities();

    assert_eq!(capabilities.dial_count(), 2);
    assert!(capabilities.clear_key());
    assert!(capabilities.clear_all());
    assert!(!capabilities.brightness());
    assert!(!capabilities.sleep());
}

#[test]
fn the_identity_is_a_name_not_an_empty_string() {
    let mut deck = SixKeyDeck::new();
    let identity = as_seam(&mut deck)
        .identity()
        .expect("this deck knows its own name");

    assert!(
        !identity.trim().is_empty(),
        "an empty identity is a success that says nothing"
    );
}
