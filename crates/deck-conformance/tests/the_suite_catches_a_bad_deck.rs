//! The suite has to catch a deck that lies.
//!
//! A conformance suite that only ever runs against a correct deck proves
//! nothing: it would pass against anything. So every case is paired with a
//! stub that breaks exactly one rule, and the test asserts that the case
//! meant to catch it does.
//!
//! The clean stub at the top of the file exists for the other direction: it
//! passes every case, which is what makes these failures mean something.

use deck_api::{
    ButtonState, Capabilities, Deck, DeckError, DeckOperation, Geometry, ImageFormat, ImageSpec,
    KeyEvent, KeyId,
};
use deck_conformance::{run, CaseResult, Outcome};
use std::time::Duration;

/// The number of keys the stub claims to have.
const KEY_COUNT: u16 = 6;

/// The one rule a stub breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flaw {
    /// Honours every rule.
    None,
    /// Accepts any key, however far past the end.
    AcceptsAnyKey,
    /// Refuses the right keys but names the wrong count.
    NamesTheWrongKeyCount,
    /// Silently clamps a brightness past the range.
    ClampsBrightness,
    /// Has no brightness, and accepts a brightness anyway.
    IgnoresMissingBrightness,
    /// Has no single-key clear, and accepts one anyway.
    IgnoresMissingClearKey,
    /// Has no clear-all, and accepts it anyway.
    IgnoresMissingClearAll,
    /// Has no sleep, and accepts it anyway.
    IgnoresMissingSleep,
    /// Has no dials, and accepts any dial.
    AcceptsAnyDial,
    /// Answers with an empty identity.
    EmptyIdentity,
    /// Declares an image nobody could paint.
    UnusableImageSpec,
}

/// A deck whose only purpose is to be caught.
struct Stub {
    flaw: Flaw,
}

impl Stub {
    fn clean() -> Self {
        Self { flaw: Flaw::None }
    }

    fn broken(flaw: Flaw) -> Self {
        Self { flaw }
    }
}

impl Deck for Stub {
    fn geometry(&self) -> Geometry {
        Geometry::new(KEY_COUNT, 3)
    }

    fn image_spec(&self) -> ImageSpec {
        if self.flaw == Flaw::UnusableImageSpec {
            return ImageSpec::new(0, 0, 0, ImageFormat::Jpeg);
        }
        ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg)
    }

    fn capabilities(&self) -> Capabilities {
        match self.flaw {
            Flaw::IgnoresMissingBrightness => Capabilities::new()
                .with_clear_key(true)
                .with_clear_all(true),
            Flaw::IgnoresMissingClearKey => Capabilities::new()
                .with_brightness(true)
                .with_clear_all(true),
            Flaw::IgnoresMissingClearAll => Capabilities::new()
                .with_brightness(true)
                .with_clear_key(true),
            _ => Capabilities::new()
                .with_brightness(true)
                .with_clear_key(true)
                .with_clear_all(true),
        }
    }

    fn identity(&self) -> Result<String, DeckError> {
        if self.flaw == Flaw::EmptyIdentity {
            return Ok(String::new());
        }
        Ok("stub deck".to_owned())
    }

    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
        if self.flaw == Flaw::ClampsBrightness || self.flaw == Flaw::IgnoresMissingBrightness {
            return Ok(());
        }
        if value > deck_api::VALUE_MAX {
            return Err(DeckError::value_out_of_range(
                DeckOperation::Brightness,
                value,
            ));
        }
        Ok(())
    }

    fn set_key_image(&mut self, key: KeyId, _jpeg: &[u8]) -> Result<(), DeckError> {
        if self.flaw == Flaw::AcceptsAnyKey {
            return Ok(());
        }
        let key_count = if self.flaw == Flaw::NamesTheWrongKeyCount {
            KEY_COUNT + 3
        } else {
            KEY_COUNT
        };
        if key.index() >= KEY_COUNT {
            return Err(DeckError::KeyOutOfRange { key, key_count });
        }
        Ok(())
    }

    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
        if self.flaw == Flaw::IgnoresMissingClearKey {
            return Ok(());
        }
        if !self.capabilities().clear_key() {
            return Err(DeckError::Unsupported {
                operation: DeckOperation::ClearKey,
            });
        }
        if key.index() >= KEY_COUNT {
            return Err(DeckError::KeyOutOfRange {
                key,
                key_count: KEY_COUNT,
            });
        }
        Ok(())
    }

    fn clear_all(&mut self) -> Result<(), DeckError> {
        if self.flaw == Flaw::IgnoresMissingClearAll {
            return Ok(());
        }
        if !self.capabilities().clear_all() {
            return Err(DeckError::Unsupported {
                operation: DeckOperation::ClearAll,
            });
        }
        Ok(())
    }

    fn sleep(&mut self) -> Result<(), DeckError> {
        if self.flaw == Flaw::IgnoresMissingSleep {
            return Ok(());
        }
        Err(DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        })
    }

    fn set_dial(&mut self, dial: u8, _value: u8) -> Result<(), DeckError> {
        if self.flaw == Flaw::AcceptsAnyDial {
            return Ok(());
        }
        let dial_count = self.capabilities().dial_count();
        if u16::from(dial) >= dial_count {
            return Err(DeckError::DialOutOfRange { dial, dial_count });
        }
        Ok(())
    }

    fn poll_events(&mut self, _timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        Ok(vec![KeyEvent::new(KeyId::FIRST, ButtonState::Released)])
    }

    fn reconnect(&mut self) -> Result<(), DeckError> {
        Ok(())
    }
}

fn outcome_of(results: &[CaseResult], name: &str) -> Outcome {
    results
        .iter()
        .find(|result| result.name == name)
        .unwrap_or_else(|| {
            panic!(
                "the suite has no case called {name}; it ran {:?}",
                results.iter().map(|result| result.name).collect::<Vec<_>>()
            )
        })
        .outcome
}

/// The case named `name` must fail against a deck with `flaw`.
fn assert_caught(flaw: Flaw, name: &str) {
    let mut deck = Stub::broken(flaw);
    let results = run(&mut deck);
    let outcome = outcome_of(&results, name);

    assert_eq!(
        outcome,
        Outcome::Fail,
        "{name} was {outcome} against a deck with {flaw:?}, so it does not catch it"
    );
}

#[test]
fn a_deck_that_honours_the_seam_passes_every_case() {
    let mut deck = Stub::clean();
    let results = run(&mut deck);

    assert!(!results.is_empty(), "the suite ran no cases at all");
    for result in &results {
        assert_ne!(
            result.outcome,
            Outcome::Fail,
            "{}: {}",
            result.name,
            result.detail
        );
    }
}

#[test]
fn the_suite_names_every_case_once() {
    let mut deck = Stub::clean();
    let results = run(&mut deck);

    let mut names: Vec<&str> = results.iter().map(|result| result.name).collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();

    assert_eq!(total, names.len(), "the suite runs a case twice");
}

#[test]
fn a_deck_that_declares_an_unpaintable_image_is_caught() {
    assert_caught(Flaw::UnusableImageSpec, "image_spec_is_usable");
}

#[test]
fn a_deck_that_accepts_any_key_is_caught() {
    assert_caught(Flaw::AcceptsAnyKey, "a_key_past_the_end_is_refused");
}

#[test]
fn a_deck_that_names_the_wrong_key_count_is_caught() {
    assert_caught(
        Flaw::NamesTheWrongKeyCount,
        "a_key_refusal_names_the_key_count_the_deck_has",
    );
}

#[test]
fn a_deck_that_clamps_brightness_is_caught() {
    assert_caught(
        Flaw::ClampsBrightness,
        "brightness_past_the_range_is_refused",
    );
}

#[test]
fn a_deck_that_ignores_its_missing_brightness_is_caught() {
    assert_caught(
        Flaw::IgnoresMissingBrightness,
        "brightness_without_the_capability_is_refused",
    );
}

#[test]
fn a_deck_that_ignores_its_missing_single_key_clear_is_caught() {
    assert_caught(
        Flaw::IgnoresMissingClearKey,
        "clear_key_without_the_capability_is_refused",
    );
}

#[test]
fn a_deck_that_ignores_its_missing_clear_all_is_caught() {
    assert_caught(
        Flaw::IgnoresMissingClearAll,
        "clear_all_without_the_capability_is_refused",
    );
}

#[test]
fn a_deck_that_ignores_its_missing_sleep_is_caught() {
    assert_caught(
        Flaw::IgnoresMissingSleep,
        "sleep_without_the_capability_is_refused",
    );
}

#[test]
fn a_deck_that_accepts_an_imaginary_dial_is_caught() {
    assert_caught(Flaw::AcceptsAnyDial, "a_dial_past_the_count_is_refused");
}

#[test]
fn a_deck_that_answers_with_an_empty_identity_is_caught() {
    assert_caught(Flaw::EmptyIdentity, "identity_is_never_an_empty_success");
}

#[test]
fn a_deck_with_a_capability_does_not_get_asked_about_the_absence_of_it() {
    let mut deck = Stub::clean();
    let results = run(&mut deck);

    for name in [
        "brightness_without_the_capability_is_refused",
        "clear_key_without_the_capability_is_refused",
        "clear_all_without_the_capability_is_refused",
    ] {
        assert_eq!(
            outcome_of(&results, name),
            Outcome::NotApplicable,
            "{name} judged a capability the deck never claimed to lack"
        );
    }
}

#[test]
fn a_deck_with_no_sleep_is_asked_for_one_and_refuses_it() {
    let mut deck = Stub::clean();
    let results = run(&mut deck);

    assert_eq!(
        outcome_of(&results, "sleep_without_the_capability_is_refused"),
        Outcome::Pass,
        "a deck that does not advertise sleep has to refuse it, not ignore it"
    );
}
