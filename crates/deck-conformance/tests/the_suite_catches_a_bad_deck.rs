//! The suite has to catch a deck that lies.
//!
//! A conformance suite that only ever runs against a correct deck proves
//! nothing: it would pass against anything. So every case is paired with a
//! stub that breaks exactly one rule, and the pairing is checked in both
//! directions:
//!
//! - each flaw fails **exactly one case, the one meant to catch it** — if two
//!   cases always fail together they are one case wearing two names;
//! - **every case in the suite has a flaw** that breaks it — a case nothing can
//!   break is a case that proves nothing.
//!
//! The clean stub exists for the third direction: it passes every case, which
//! is what makes the failures mean something.

use std::time::Duration;

use deck_api::{
    ButtonState, Capabilities, Deck, DeckError, DeckOperation, Geometry, ImageFormat, ImageSpec,
    KeyEvent, KeyId,
};
use deck_conformance::{run, CaseResult, Outcome};

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
    /// Accepts an image of any length.
    AcceptsAnyImage,
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

/// Every flaw, and the single case it has to fail.
const PAIRS: &[(Flaw, &str)] = &[
    (Flaw::UnusableImageSpec, "image_spec_is_usable"),
    (Flaw::AcceptsAnyKey, "a_key_past_the_end_is_refused"),
    (Flaw::NamesTheWrongKeyCount, "a_key_past_the_end_is_refused"),
    (Flaw::AcceptsAnyImage, "an_image_over_the_limit_is_refused"),
    (
        Flaw::ClampsBrightness,
        "brightness_past_the_range_is_refused",
    ),
    (
        Flaw::IgnoresMissingBrightness,
        "brightness_without_the_capability_is_refused",
    ),
    (
        Flaw::IgnoresMissingClearKey,
        "clear_key_without_the_capability_is_refused",
    ),
    (
        Flaw::IgnoresMissingClearAll,
        "clear_all_without_the_capability_is_refused",
    ),
    (
        Flaw::IgnoresMissingSleep,
        "sleep_without_the_capability_is_refused",
    ),
    (Flaw::AcceptsAnyDial, "a_dial_past_the_count_is_refused"),
    (Flaw::EmptyIdentity, "identity_is_never_an_empty_success"),
];

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

    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        if key.index() >= KEY_COUNT && self.flaw != Flaw::AcceptsAnyKey {
            let key_count = if self.flaw == Flaw::NamesTheWrongKeyCount {
                KEY_COUNT + 3
            } else {
                KEY_COUNT
            };
            return Err(DeckError::KeyOutOfRange { key, key_count });
        }

        let max_bytes = self.image_spec().max_bytes();
        if jpeg.len() > max_bytes && self.flaw != Flaw::AcceptsAnyImage {
            return Err(DeckError::ImageTooLarge {
                bytes: jpeg.len(),
                max_bytes,
            });
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

fn failures(results: &[CaseResult]) -> Vec<&'static str> {
    results
        .iter()
        .filter(|result| result.outcome == Outcome::Fail)
        .map(|result| result.name)
        .collect()
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
fn each_flaw_fails_exactly_the_case_meant_to_catch_it() {
    let mut problems = Vec::new();

    for (flaw, name) in PAIRS {
        let mut deck = Stub::broken(*flaw);
        let failed = failures(&run(&mut deck));
        if failed != vec![*name] {
            problems.push(format!(
                "{flaw:?} failed {failed:?}, but it should fail exactly [{name:?}]"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "the pairing between flaws and cases is wrong:\n{}",
        problems.join("\n")
    );
}

#[test]
fn every_case_in_the_suite_is_paired_with_a_flaw() {
    let mut deck = Stub::clean();
    let results = run(&mut deck);

    let mut names: Vec<&str> = results.iter().map(|result| result.name).collect();
    let mut paired: Vec<&str> = PAIRS.iter().map(|(_, name)| *name).collect();
    names.sort_unstable();
    names.dedup();
    paired.sort_unstable();
    paired.dedup();

    assert_eq!(
        names, paired,
        "a case exists that no stub can break, or a stub names a case that does not exist"
    );
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
