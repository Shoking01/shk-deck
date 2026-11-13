//! The handle forwards everything to the deck inside it.
//!
//! The handle is the only thing the window and the engine will ever hold, so
//! a forwarding mistake here is a mistake the whole application inherits.
//! These tests use a stub deck that records what it was asked to do and can
//! be told to refuse, so both directions are covered: a call that reaches the
//! deck, and a refusal that comes back out of it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use deck_api::{
    ButtonState, Capabilities, Deck, DeckError, DeckHandle, DeckOperation, Geometry, ImageFormat,
    ImageSpec, KeyEvent, KeyId,
};

/// The calls the stub was asked to make, in order.
type Calls = Rc<RefCell<Vec<String>>>;

/// A deck that records every call and answers with whatever the test needs.
struct StubDeck {
    calls: Calls,
    /// When true, every fallible call refuses with its own operation named.
    refuse: bool,
}

impl StubDeck {
    fn new() -> Self {
        Self {
            calls: Rc::new(RefCell::new(Vec::new())),
            refuse: false,
        }
    }

    fn with_calls(calls: &Calls) -> Self {
        Self {
            calls: Rc::clone(calls),
            refuse: false,
        }
    }

    fn refusing(calls: &Calls) -> Self {
        Self {
            calls: Rc::clone(calls),
            refuse: true,
        }
    }

    fn record(&self, call: impl Into<String>) {
        self.calls.borrow_mut().push(call.into());
    }

    fn refusal(&self, operation: DeckOperation) -> Result<(), DeckError> {
        if self.refuse {
            Err(DeckError::Unsupported { operation })
        } else {
            Ok(())
        }
    }
}

impl Deck for StubDeck {
    fn geometry(&self) -> Geometry {
        Geometry::new(6, 3)
    }

    fn image_spec(&self) -> ImageSpec {
        ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::new()
            .with_brightness(true)
            .with_clear_key(true)
            .with_clear_all(true)
            .with_keep_alive(true)
    }

    fn identity(&self) -> Result<String, DeckError> {
        self.record("identity");
        self.refusal(DeckOperation::Identity)?;
        Ok("stub deck".to_owned())
    }

    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
        self.record(format!("set_brightness({value})"));
        self.refusal(DeckOperation::Brightness)
    }

    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        self.record(format!(
            "set_key_image({}, {} bytes)",
            key.index(),
            jpeg.len()
        ));
        self.refusal(DeckOperation::SetKeyImage)
    }

    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
        self.record(format!("clear_key({})", key.index()));
        self.refusal(DeckOperation::ClearKey)
    }

    fn clear_all(&mut self) -> Result<(), DeckError> {
        self.record("clear_all");
        self.refusal(DeckOperation::ClearAll)
    }

    fn sleep(&mut self) -> Result<(), DeckError> {
        self.record("sleep");
        self.refusal(DeckOperation::Sleep)
    }

    fn keep_alive(&mut self) -> Result<(), DeckError> {
        self.record("keep_alive");
        self.refusal(DeckOperation::KeepAlive)
    }

    fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError> {
        self.record(format!("set_dial({dial}, {value})"));
        self.refusal(DeckOperation::SetDial)
    }

    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        self.record(format!("poll_events({timeout:?})"));
        self.refusal(DeckOperation::PollEvents)?;
        Ok(vec![KeyEvent::new(KeyId::new(1), ButtonState::Pressed)])
    }

    fn reconnect(&mut self) -> Result<(), DeckError> {
        self.record("reconnect");
        self.refusal(DeckOperation::Reconnect)
    }
}

#[test]
fn the_handle_forwards_every_call_to_the_deck_inside_it() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut handle = DeckHandle::new(Box::new(StubDeck::with_calls(&calls)));

    handle.identity().expect("the stub answers");
    handle.set_brightness(40).expect("the stub accepts");
    handle
        .set_key_image(KeyId::new(2), &[1, 2, 3])
        .expect("the stub accepts");
    handle.clear_key(KeyId::new(2)).expect("the stub accepts");
    handle.clear_all().expect("the stub accepts");
    handle.sleep().expect("the stub accepts");
    handle.keep_alive().expect("the stub accepts");
    handle.set_dial(0, 7).expect("the stub accepts");
    handle
        .poll_events(Duration::from_millis(5))
        .expect("the stub accepts");
    handle.reconnect().expect("the stub accepts");

    assert_eq!(
        calls.borrow().as_slice(),
        [
            "identity",
            "set_brightness(40)",
            "set_key_image(2, 3 bytes)",
            "clear_key(2)",
            "clear_all",
            "sleep",
            "keep_alive",
            "set_dial(0, 7)",
            "poll_events(5ms)",
            "reconnect",
        ]
    );
}

#[test]
fn the_handle_answers_with_the_image_spec_of_the_deck_inside_it() {
    let handle = DeckHandle::new(Box::new(StubDeck::new()));

    assert_eq!(
        handle.image_spec(),
        ImageSpec::new(72, 72, 4_096, ImageFormat::Jpeg)
    );
}

#[test]
fn the_handle_answers_with_the_capabilities_of_the_deck_inside_it() {
    let handle = DeckHandle::new(Box::new(StubDeck::new()));

    let capabilities = handle.capabilities();
    assert!(capabilities.brightness());
    assert!(capabilities.clear_key());
    assert!(capabilities.clear_all());
    assert!(!capabilities.sleep());
    assert!(capabilities.keep_alive());
    assert_eq!(capabilities.dial_count(), 0);
}

#[test]
fn the_handle_returns_what_the_deck_inside_it_returned() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut handle = DeckHandle::new(Box::new(StubDeck::with_calls(&calls)));

    let events = handle
        .poll_events(Duration::from_millis(5))
        .expect("the stub accepts");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].key().index(), 1);
    assert_eq!(events[0].state(), ButtonState::Pressed);
}

#[test]
fn a_refusal_from_the_deck_reaches_the_caller_naming_its_operation() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut handle = DeckHandle::new(Box::new(StubDeck::refusing(&calls)));

    let error = handle.sleep().expect_err("the stub refuses to sleep");

    assert_eq!(
        error,
        DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        }
    );
    assert_eq!(error.to_string(), "sleep is not supported by this deck");
}

#[test]
fn a_refusal_does_not_swallow_the_call_it_refused() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let mut handle = DeckHandle::new(Box::new(StubDeck::refusing(&calls)));

    let _ = handle.set_brightness(10);
    let _ = handle.sleep();

    assert_eq!(calls.borrow().as_slice(), ["set_brightness(10)", "sleep"]);
}

#[test]
fn the_handle_answers_with_the_geometry_of_the_deck_inside_it() {
    let handle = DeckHandle::new(Box::new(StubDeck::new()));

    assert_eq!(handle.geometry().key_count(), 6);
    assert_eq!(handle.geometry().rows(), 2);
    assert_eq!(handle.geometry().columns(), 3);
}

#[test]
fn an_unsupported_operation_names_itself() {
    let error = DeckError::Unsupported {
        operation: DeckOperation::Sleep,
    };

    assert_eq!(error.to_string(), "sleep is not supported by this deck");
}

#[test]
fn a_value_out_of_range_names_the_operation_and_the_range() {
    let error = DeckError::value_out_of_range(DeckOperation::Brightness, 220);

    assert_eq!(
        error.to_string(),
        "brightness value 220 is out of range (expected 0 to 100)"
    );
}

#[test]
fn a_dial_that_does_not_exist_says_how_many_there_are() {
    let error = DeckError::DialOutOfRange {
        dial: 3,
        dial_count: 2,
    };

    assert_eq!(
        error.to_string(),
        "dial 3 does not exist (this deck has 2 dials)"
    );
}

#[test]
fn a_key_that_does_not_exist_says_how_many_there_are() {
    let error = DeckError::KeyOutOfRange {
        key: KeyId::new(9),
        key_count: 6,
    };

    assert_eq!(
        error.to_string(),
        "key index 9 is out of range (this deck has 6 keys)"
    );
}

#[test]
fn an_image_that_is_too_big_says_the_limit() {
    let error = DeckError::ImageTooLarge {
        bytes: 9_000,
        max_bytes: 4_096,
    };

    assert_eq!(
        error.to_string(),
        "image is 9000 bytes; this deck accepts at most 4096"
    );
}

#[test]
fn an_image_of_the_wrong_size_says_the_expected_one() {
    let error = DeckError::ImageWrongSize {
        width: 72,
        height: 72,
        expected_width: 100,
        expected_height: 100,
    };

    assert_eq!(error.to_string(), "image is 72x72; this deck takes 100x100");
}
