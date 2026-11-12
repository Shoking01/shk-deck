//! The cases every deck has to answer.
//!
//! Two rules shaped this file, and both of them are about honesty:
//!
//! **Every case is falsifiable through the seam.** A case that no deck can
//! fail is a case that proves nothing, so each one here is paired with a stub
//! in `tests/the_suite_catches_a_bad_deck.rs` that breaks exactly that rule and
//! must make exactly that case fail. If a case cannot be broken, it does not
//! belong here.
//!
//! **No case changes the deck.** The same suite runs against a real device on
//! a desk, so the only calls it makes are the ones a deck is obliged to
//! refuse. A deck that passes has had nothing painted on it.

use deck_api::{Deck, DeckError, DeckOperation, KeyId, VALUE_MAX};

use crate::CaseResult;

/// Every case, in a fixed order so a report reads as a stable list.
pub(crate) fn all(deck: &mut dyn Deck) -> Vec<CaseResult> {
    vec![
        image_spec_is_usable(deck),
        a_key_past_the_end_is_refused(deck),
        a_key_refusal_names_the_key_count_the_deck_has(deck),
        brightness_past_the_range_is_refused(deck),
        brightness_without_the_capability_is_refused(deck),
        clear_key_without_the_capability_is_refused(deck),
        clear_all_without_the_capability_is_refused(deck),
        sleep_without_the_capability_is_refused(deck),
        a_dial_past_the_count_is_refused(deck),
        identity_is_never_an_empty_success(deck),
    ]
}

/// Asserts that `result` refused with exactly `expected`.
fn expect_refusal<T>(
    name: &'static str,
    result: Result<T, DeckError>,
    expected: DeckError,
) -> CaseResult {
    match result {
        Err(actual) if actual == expected => CaseResult::pass(name),
        Err(actual) => CaseResult::fail(name, format!("expected {expected}, got {actual}")),
        Ok(_) => CaseResult::fail(name, format!("expected {expected}, got success")),
    }
}

/// A deck that cannot say what an image looks like cannot be painted.
fn image_spec_is_usable(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "image_spec_is_usable";

    let spec = deck.image_spec();
    if spec.width() == 0 || spec.height() == 0 {
        return CaseResult::fail(
            NAME,
            format!(
                "the deck declares a {}x{} image, which no image can match",
                spec.width(),
                spec.height()
            ),
        );
    }
    if spec.max_bytes() == 0 {
        return CaseResult::fail(NAME, "the deck declares a limit of zero bytes for an image");
    }
    CaseResult::pass(NAME)
}

/// The key range is checked before the payload.
///
/// The payload here is empty, and that is deliberate: a deck that inspects the
/// bytes before the key answers with an image error, which is the wrong
/// answer and a wasted decode. Refusing the key first is cheaper and it is the
/// only thing the suite can require without knowing how to encode an image.
fn a_key_past_the_end_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "a_key_past_the_end_is_refused";

    let key_count = deck.geometry().key_count();
    let Some(past_the_end) = key_count.checked_add(1) else {
        return CaseResult::not_applicable(
            NAME,
            "the deck already names every key a u16 can address",
        );
    };

    let key = KeyId::new(past_the_end);
    expect_refusal(
        NAME,
        deck.set_key_image(key, &[]),
        DeckError::KeyOutOfRange { key, key_count },
    )
}

/// A refusal is only useful if it reports the deck you are actually holding.
///
/// This is the case that catches the deck which grew out of a fifteen-key
/// shape and still names fifteen in its errors while declaring six.
fn a_key_refusal_names_the_key_count_the_deck_has(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "a_key_refusal_names_the_key_count_the_deck_has";

    let declared = deck.geometry().key_count();
    let Some(past_the_end) = declared.checked_add(1) else {
        return CaseResult::not_applicable(
            NAME,
            "the deck already names every key a u16 can address",
        );
    };

    match deck.set_key_image(KeyId::new(past_the_end), &[]) {
        Err(DeckError::KeyOutOfRange { key_count, .. }) if key_count == declared => {
            CaseResult::pass(NAME)
        }
        Err(DeckError::KeyOutOfRange { key_count, .. }) => CaseResult::fail(
            NAME,
            format!("the deck reports {key_count} keys but declares {declared}"),
        ),
        Err(other) => CaseResult::fail(NAME, format!("expected a key refusal, got {other}")),
        Ok(()) => CaseResult::fail(NAME, "the deck accepted a key past the end"),
    }
}

/// One past the top of the range is refused, not clamped.
fn brightness_past_the_range_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "brightness_past_the_range_is_refused";

    if !deck.capabilities().brightness() {
        return CaseResult::not_applicable(NAME, "this deck has no brightness");
    }

    let value = VALUE_MAX + 1;
    expect_refusal(
        NAME,
        deck.set_brightness(value),
        DeckError::value_out_of_range(DeckOperation::Brightness, value),
    )
}

/// A capability the deck does not advertise is refused, never ignored.
fn brightness_without_the_capability_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "brightness_without_the_capability_is_refused";

    if deck.capabilities().brightness() {
        return CaseResult::not_applicable(NAME, "this deck has brightness");
    }

    expect_refusal(
        NAME,
        deck.set_brightness(50),
        DeckError::Unsupported {
            operation: DeckOperation::Brightness,
        },
    )
}

/// Clearing one key, when the deck does not advertise it.
fn clear_key_without_the_capability_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "clear_key_without_the_capability_is_refused";

    if deck.capabilities().clear_key() {
        return CaseResult::not_applicable(NAME, "this deck clears single keys");
    }
    if !deck.geometry().contains(KeyId::FIRST) {
        return CaseResult::not_applicable(NAME, "this deck has no keys to name");
    }

    expect_refusal(
        NAME,
        deck.clear_key(KeyId::FIRST),
        DeckError::Unsupported {
            operation: DeckOperation::ClearKey,
        },
    )
}

/// Clearing every key, when the deck does not advertise it.
fn clear_all_without_the_capability_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "clear_all_without_the_capability_is_refused";

    if deck.capabilities().clear_all() {
        return CaseResult::not_applicable(NAME, "this deck clears every key");
    }

    expect_refusal(
        NAME,
        deck.clear_all(),
        DeckError::Unsupported {
            operation: DeckOperation::ClearAll,
        },
    )
}

/// Sleeping, when the deck does not advertise it.
fn sleep_without_the_capability_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "sleep_without_the_capability_is_refused";

    if deck.capabilities().sleep() {
        return CaseResult::not_applicable(NAME, "this deck sleeps");
    }

    expect_refusal(
        NAME,
        deck.sleep(),
        DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        },
    )
}

/// One past the last dial is refused, and with the count of dials there are.
fn a_dial_past_the_count_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "a_dial_past_the_count_is_refused";

    let dial_count = deck.capabilities().dial_count();
    let Ok(dial) = u8::try_from(dial_count) else {
        return CaseResult::not_applicable(NAME, "the deck has more dials than a u8 can address");
    };

    expect_refusal(
        NAME,
        deck.set_dial(dial, 0),
        DeckError::DialOutOfRange { dial, dial_count },
    )
}

/// An empty name is not an identity.
///
/// A deck that cannot read its identity is free to refuse: the failure is
/// honest. What it may not do is answer with nothing and call it success,
/// because every consumer will then display an empty string.
fn identity_is_never_an_empty_success(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "identity_is_never_an_empty_success";

    match deck.identity() {
        Ok(name) if name.trim().is_empty() => {
            CaseResult::fail(NAME, "the deck answered with an empty name")
        }
        _ => CaseResult::pass(NAME),
    }
}
