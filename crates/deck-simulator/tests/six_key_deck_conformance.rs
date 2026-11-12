//! The shared suite, run against the second deck.
//!
//! This crate's whole reason to exist is that the seam is proven by something
//! that is not the deck it was written for. So the proof is not a bespoke test:
//! it is `deck_conformance::run`, the same file that runs against the device.
//!
//! `NotApplicable` is a legitimate answer here, because this deck deliberately
//! lacks brightness. Only `Fail` is a defect, and the failure detail is printed
//! so a broken run says which rule broke rather than "one case failed".

use deck_api::Deck;
use deck_conformance::Outcome;
use deck_simulator::SixKeyDeck;

#[test]
fn the_six_key_deck_passes_the_shared_conformance_suite() {
    let mut deck = SixKeyDeck::new();

    let results = deck_conformance::run(&mut deck);

    assert!(
        !results.is_empty(),
        "the suite ran no cases at all, so nothing was proven"
    );

    let failures: Vec<String> = results
        .iter()
        .filter(|case| case.outcome == Outcome::Fail)
        .map(|case| format!("{}: {}", case.name, case.detail))
        .collect();

    assert!(
        failures.is_empty(),
        "failed cases:\n{}",
        failures.join("\n")
    );
}

#[test]
fn the_suite_judges_every_operation_this_deck_declares() {
    let mut deck = SixKeyDeck::new();
    let results = deck_conformance::run(&mut deck);

    // The two refusal cases the deck must answer as a deck, not as a stub with
    // another model's constants: the key count and the payload limit in the
    // error have to be this deck's numbers. The dial and identity cases are
    // here because this deck advertises both.
    for name in [
        "a_key_past_the_end_is_refused",
        "an_image_over_the_limit_is_refused",
        "a_dial_past_the_count_is_refused",
        "identity_is_never_an_empty_success",
    ] {
        let case = results
            .iter()
            .find(|case| case.name == name)
            .unwrap_or_else(|| panic!("the suite did not run {name}"));
        assert_eq!(
            case.outcome,
            Outcome::Pass,
            "{name} did not pass: {}",
            case.detail
        );
    }
}

#[test]
fn the_suite_skips_brightness_because_the_deck_never_claims_it() {
    let mut deck = SixKeyDeck::new();
    assert!(!deck.capabilities().brightness());

    let results = deck_conformance::run(&mut deck);
    let brightness = results
        .iter()
        .find(|case| case.name == "brightness_past_the_range_is_refused")
        .expect("the suite runs the brightness range case");

    assert_eq!(
        brightness.outcome,
        Outcome::NotApplicable,
        "a deck with no brightness must not be judged on the brightness range"
    );

    // The absence itself is judged, in the case that asks for a brightness the
    // deck never advertised. It has to be a typed refusal, not a success.
    let missing = results
        .iter()
        .find(|case| case.name == "brightness_without_the_capability_is_refused")
        .expect("the suite runs the missing-capability case");
    assert_eq!(
        missing.outcome,
        Outcome::Pass,
        "a deck with no brightness must refuse it: {}",
        missing.detail
    );
}
