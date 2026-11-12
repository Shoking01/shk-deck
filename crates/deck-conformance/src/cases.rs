//! The cases every deck has to answer.
//!
//! Three rules shaped this file, and all three are about honesty.
//!
//! **Every case is falsifiable, and independent.** A case no deck can fail
//! proves nothing, and two cases that the same deck always fails together are
//! one case wearing two names. Each case here is paired with a stub in
//! `tests/the_suite_catches_a_bad_deck.rs` that breaks exactly that rule, and
//! the test asserts that stub fails exactly that case and no other.
//!
//! **No case paints a key.** The same suite runs against a real device on a
//! desk, so it only makes calls it expects to be refused. A successful paint is
//! the provider's own test to write.
//!
//! **The cheap check comes first.** A deck that inspects a payload before it
//! checks the key, or decodes before it checks the limit, spends work on a
//! question it could have answered for free — and on hardware that work is a
//! write waiting to happen.

use deck_api::{Deck, DeckError, DeckOperation, KeyId, VALUE_MAX};

use crate::CaseResult;

/// Every case, in a fixed order so a report reads as a stable list.
pub(crate) fn all(deck: &mut dyn Deck) -> Vec<CaseResult> {
    vec![
        image_spec_is_usable(deck),
        a_key_past_the_end_is_refused(deck),
        an_image_over_the_limit_is_refused(deck),
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

/// A key past the end is refused, naming the key count the deck declares.
///
/// The payload is empty on purpose: a deck that inspects the bytes before the
/// key answers with an image error, which is the wrong answer and a wasted
/// decode. The expected error carries the declared count as well as the key, so
/// a deck that grew out of another shape and still reports the old number is
/// caught here rather than in a test of its own.
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

/// One byte over the declared limit is refused, naming the limit.
///
/// The key is one the deck has, so a deck that refuses everything cannot pass
/// this, and a deck that accepts anything cannot pass the case before it. The
/// payload is a run of bytes rather than an image: only the length matters, and
/// checking it needs no decoder — which is exactly why it belongs first.
fn an_image_over_the_limit_is_refused(deck: &mut dyn Deck) -> CaseResult {
    const NAME: &str = "an_image_over_the_limit_is_refused";

    if !deck.geometry().contains(KeyId::FIRST) {
        return CaseResult::not_applicable(NAME, "this deck has no keys to paint");
    }

    let max_bytes = deck.image_spec().max_bytes();
    let Some(bytes) = max_bytes.checked_add(1) else {
        return CaseResult::not_applicable(NAME, "the deck accepts every image a usize can count");
    };

    let payload = vec![0_u8; bytes];
    expect_refusal(
        NAME,
        deck.set_key_image(KeyId::FIRST, &payload),
        DeckError::ImageTooLarge { bytes, max_bytes },
    )
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
