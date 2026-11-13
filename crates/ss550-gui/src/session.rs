//! The deck in front of the window, as the window needs it.
//!
//! The window does not know which deck it is driving. The grid, the image
//! size and the offerable controls all come from the [`Deck`] behind this
//! session, and an operation the deck does not have comes back as its own
//! typed error instead of a silent no-op.
//!
//! Keeping that knowledge here — instead of in the render code — is what makes
//! it testable with no device: the tests drive a real `Deck` implementation
//! (`deck_simulator::SixKeyDeck`) through the same seam the hardware uses.

use std::time::Duration;

use deck_api::{Capabilities, Deck, DeckError, DeckHandle, Geometry, ImageSpec, KeyEvent, KeyId};
use jpeg_encoder::{ColorType, Encoder};

/// The JPEG quality used for the key images.
///
/// High enough that a solid colour and its label stay clean, low enough that a
/// full-size key is far below every payload limit the seam knows about.
const QUALITY: u8 = 80;

/// Why the window could not do what it was asked to.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The deck refused the operation, with its own typed reason.
    #[error(transparent)]
    Deck(#[from] DeckError),

    /// The key is not on this deck.
    #[error("key {key} does not exist on this deck (it has {key_count} keys)")]
    NoSuchKey {
        /// The key that was asked for, in the deck's own 0-based order.
        key: u16,
        /// How many keys the deck actually has.
        key_count: u16,
    },

    /// The tile could not be encoded.
    #[error("could not encode the key image: {0}")]
    Image(#[from] jpeg_encoder::EncodingError),
}

/// One deck, and everything the window asks it about.
///
/// The session is built once per connection. It reads the deck's geometry,
/// image spec and capabilities at construction, so every later question is
/// answered the same way for the lifetime of the connection: the window never
/// re-derives a key count on its own.
pub struct DeckSession {
    deck: DeckHandle,
    /// What this deck is called: its identity, or its shape when it has none.
    label: String,
    /// The stable name of this deck's bindings file.
    key: String,
    geometry: Geometry,
    image_spec: ImageSpec,
    capabilities: Capabilities,
    /// Key indices in display order, top row first.
    rows: Vec<Vec<usize>>,
}

impl DeckSession {
    /// Reads the deck's shape and capabilities, and keeps the deck.
    ///
    /// A deck that cannot report an identity is not an error: the shape it
    /// declares is a perfectly good name, and it is the only name a deck can
    /// have before it is asked.
    ///
    /// ```text
    /// let session = DeckSession::new(Box::new(SixKeyDeck::new()));
    /// assert_eq!(session.geometry().key_count(), 6);
    /// ```
    pub fn new(deck: Box<dyn Deck>) -> Self {
        let geometry = deck.geometry();
        let image_spec = deck.image_spec();
        let capabilities = deck.capabilities();

        let identity = deck
            .identity()
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty());

        let shape = shape_label(geometry, image_spec);
        let label = identity.clone().unwrap_or_else(|| shape.clone());
        let key = identity.unwrap_or_else(|| format!("shape: {shape}"));

        Self {
            deck: DeckHandle::new(deck),
            label,
            key,
            geometry,
            image_spec,
            capabilities,
            rows: display_rows(geometry),
        }
    }

    /// What the deck is called, for the window's texts.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The stable name of this deck's bindings file.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The shape of the deck's keys.
    pub fn geometry(&self) -> Geometry {
        self.geometry
    }

    /// The image the deck accepts for one key.
    pub fn image_spec(&self) -> ImageSpec {
        self.image_spec
    }

    /// What the deck can do.
    pub fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    /// How many keys the deck has.
    pub fn key_count(&self) -> usize {
        usize::from(self.geometry.key_count())
    }

    /// The keys in display order, top row first.
    ///
    /// The deck's own geometry counts its first row from its own origin, which
    /// on the hardware is the bottom row. The window shows the deck the way it
    /// sits on the desk, so the rows are reversed here, once, instead of in
    /// every piece of render code.
    pub fn rows(&self) -> &[Vec<usize>] {
        &self.rows
    }

    /// Paints one key, at the size and with the payload limit the deck states.
    ///
    /// The key is checked before anything is encoded: a key the deck does not
    /// have is refused without spending an image on it.
    pub fn paint(&mut self, index: usize, colour: [u8; 3]) -> Result<(), SessionError> {
        let key = self.existing_key(index)?;
        let jpeg = encode_tile(self.image_spec, colour)?;
        self.deck.set_key_image(key, &jpeg)?;
        Ok(())
    }

    /// Blanks one key.
    pub fn clear_key(&mut self, index: usize) -> Result<(), SessionError> {
        let key = self.existing_key(index)?;
        self.deck.clear_key(key)?;
        Ok(())
    }

    /// Blanks every key at once.
    pub fn clear_all(&mut self) -> Result<(), DeckError> {
        self.deck.clear_all()
    }

    /// Sets the display brightness.
    ///
    /// A deck without brightness refuses with
    /// [`DeckError::Unsupported`], and the window reports that refusal instead
    /// of pretending the click did something.
    pub fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
        self.deck.set_brightness(value)
    }

    /// Keeps the deck from resetting itself while the host is idle.
    pub fn keep_alive(&mut self) -> Result<(), DeckError> {
        self.deck.keep_alive()
    }

    /// Reads every key event waiting on the deck.
    pub fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        self.deck.poll_events(timeout)
    }

    /// The deck's own id for a key the window shows, or the typed refusal.
    fn existing_key(&self, index: usize) -> Result<KeyId, SessionError> {
        let key = u16::try_from(index).unwrap_or(u16::MAX);
        if !self.geometry.contains(KeyId::new(key)) {
            return Err(SessionError::NoSuchKey {
                key,
                key_count: self.geometry.key_count(),
            });
        }
        Ok(KeyId::new(key))
    }
}

/// The key indices of a geometry, top row first.
fn display_rows(geometry: Geometry) -> Vec<Vec<usize>> {
    let columns = usize::from(geometry.columns()).max(1);
    let count = usize::from(geometry.key_count());
    let mut rows: Vec<Vec<usize>> = (0..count)
        .collect::<Vec<_>>()
        .chunks(columns)
        .map(<[usize]>::to_vec)
        .collect();
    rows.reverse();
    rows
}

/// What a deck with no identity of its own is called.
fn shape_label(geometry: Geometry, image_spec: ImageSpec) -> String {
    format!(
        "{} keys · {} columns · {}x{} images",
        geometry.key_count(),
        geometry.columns(),
        image_spec.width(),
        image_spec.height()
    )
}

/// Encodes one solid-colour tile at the size the deck accepts.
///
/// The size comes from the deck's [`ImageSpec`], never from a constant: a deck
/// that takes square images and a deck that takes wide ones are painted by the
/// same code.
pub fn encode_tile(spec: ImageSpec, colour: [u8; 3]) -> Result<Vec<u8>, SessionError> {
    let width = spec.width();
    let height = spec.height();
    let pixels = usize::from(width) * usize::from(height);
    let mut rgb = Vec::with_capacity(pixels * 3);
    for _ in 0..pixels {
        rgb.extend_from_slice(&colour);
    }

    let mut jpeg = Vec::new();
    Encoder::new(&mut jpeg, QUALITY).encode(&rgb, width, height, ColorType::Rgb)?;
    Ok(jpeg)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use deck_api::{ButtonState, DeckOperation, KeyEvent};
    use deck_simulator::SixKeyDeck;

    use super::*;

    /// Every call the window made to the deck, in order.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Paint(KeyId, Vec<u8>),
        ClearKey(KeyId),
        ClearAll,
        Brightness(u8),
        KeepAlive,
        Poll,
    }

    /// The record of what the window asked the deck to do.
    type Calls = Rc<RefCell<Vec<Call>>>;

    /// The key transitions the deck will report, shared with the test.
    type Events = Rc<RefCell<Vec<KeyEvent>>>;

    /// A deck that records what it was asked to do before doing it.
    ///
    /// The seam owns the deck once the session is built, so this is how a test
    /// keeps watching: the record lives outside, and the deck inside is the
    /// real `SixKeyDeck`, not a second fake. The key queue is shared for the
    /// same reason: `SixKeyDeck::queue_event` would otherwise be unreachable
    /// once the session owns the box.
    struct Recording {
        inner: SixKeyDeck,
        calls: Calls,
        events: Events,
    }

    impl Recording {
        fn new() -> (Self, Calls, Events) {
            let calls = Rc::new(RefCell::new(Vec::new()));
            let events = Rc::new(RefCell::new(Vec::new()));
            (
                Self {
                    inner: SixKeyDeck::new(),
                    calls: Rc::clone(&calls),
                    events: Rc::clone(&events),
                },
                calls,
                events,
            )
        }

        fn record(&self, call: Call) {
            self.calls.borrow_mut().push(call);
        }
    }

    impl Deck for Recording {
        fn geometry(&self) -> Geometry {
            self.inner.geometry()
        }

        fn image_spec(&self) -> ImageSpec {
            self.inner.image_spec()
        }

        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        fn identity(&self) -> Result<String, DeckError> {
            self.inner.identity()
        }

        fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
            self.record(Call::Brightness(value));
            self.inner.set_brightness(value)
        }

        fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
            self.record(Call::Paint(key, jpeg.to_vec()));
            self.inner.set_key_image(key, jpeg)
        }

        fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
            self.record(Call::ClearKey(key));
            self.inner.clear_key(key)
        }

        fn clear_all(&mut self) -> Result<(), DeckError> {
            self.record(Call::ClearAll);
            self.inner.clear_all()
        }

        fn sleep(&mut self) -> Result<(), DeckError> {
            self.inner.sleep()
        }

        fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError> {
            self.inner.set_dial(dial, value)
        }

        fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
            self.record(Call::Poll);
            let _ = timeout;
            Ok(self.events.borrow_mut().drain(..).collect())
        }

        fn keep_alive(&mut self) -> Result<(), DeckError> {
            self.record(Call::KeepAlive);
            self.inner.keep_alive()
        }

        fn reconnect(&mut self) -> Result<(), DeckError> {
            self.inner.reconnect()
        }
    }

    fn session() -> (DeckSession, Calls, Events) {
        let (deck, calls, events) = Recording::new();
        (DeckSession::new(Box::new(deck)), calls, events)
    }

    #[test]
    fn the_six_key_deck_shapes_the_whole_session() {
        let (session, _, _) = session();

        assert_eq!(session.key_count(), 6);
        assert_eq!(session.geometry().columns(), 3);
        assert_eq!(session.geometry().rows(), 2);
        assert_eq!(session.image_spec().width(), 72);
        assert_eq!(session.image_spec().height(), 72);
        assert_eq!(session.image_spec().max_bytes(), 4_096);
        assert!(session.capabilities().clear_all());
        assert!(session.capabilities().clear_key());
        assert!(!session.capabilities().brightness());
    }

    #[test]
    fn the_grid_is_shown_top_row_first() {
        let (session, _, _) = session();

        assert_eq!(session.rows(), vec![vec![3, 4, 5], vec![0, 1, 2]]);
    }

    #[test]
    fn a_tile_is_encoded_at_the_decks_own_size() {
        let (mut session, calls, _) = session();

        session.paint(0, [0xDC, 0x26, 0x26]).expect("key 0 exists");

        let calls = calls.borrow();
        let Call::Paint(key, jpeg) = &calls[0] else {
            panic!("expected a paint, got {:?}", calls[0]);
        };
        assert_eq!(*key, KeyId::new(0));
        assert!(jpeg.len() <= session.image_spec().max_bytes());
        assert_eq!(jpeg_size(jpeg), (72, 72));
    }

    #[test]
    fn a_key_the_deck_does_not_have_is_refused_before_encoding() {
        let (mut session, calls, _) = session();

        let error = session
            .paint(6, [0xFF, 0xFF, 0xFF])
            .expect_err("the six-key deck has no key 6");

        assert_eq!(
            error.to_string(),
            "key 6 does not exist on this deck (it has 6 keys)"
        );
        assert!(calls.borrow().is_empty(), "nothing must reach the deck");
    }

    #[test]
    fn brightness_is_a_typed_refusal_on_a_deck_without_it() {
        let (mut session, calls, _) = session();

        let error = session
            .set_brightness(60)
            .expect_err("this deck has no brightness");

        assert_eq!(
            error,
            DeckError::Unsupported {
                operation: DeckOperation::Brightness
            }
        );
        assert_eq!(calls.borrow().as_slice(), &[Call::Brightness(60)]);
    }

    #[test]
    fn keep_alive_is_a_typed_refusal_on_a_deck_that_never_resets() {
        let (mut session, calls, _) = session();

        let error = session
            .keep_alive()
            .expect_err("this deck never resets, so it is never kept awake");

        assert_eq!(
            error,
            DeckError::Unsupported {
                operation: DeckOperation::KeepAlive
            }
        );
        assert_eq!(calls.borrow().as_slice(), &[Call::KeepAlive]);
    }

    #[test]
    fn a_press_on_the_deck_reaches_the_window_as_an_event() {
        let (mut session, calls, events) = session();
        events
            .borrow_mut()
            .push(KeyEvent::new(KeyId::new(2), ButtonState::Pressed));

        let pressed = session
            .poll_events(Duration::ZERO)
            .expect("the deck answers");

        assert_eq!(
            pressed,
            vec![KeyEvent::new(KeyId::new(2), ButtonState::Pressed)]
        );
        assert_eq!(calls.borrow().as_slice(), &[Call::Poll]);
    }

    #[test]
    fn a_deck_without_an_identity_is_named_by_its_shape() {
        struct Anonymous(SixKeyDeck);

        impl Deck for Anonymous {
            fn geometry(&self) -> Geometry {
                self.0.geometry()
            }
            fn image_spec(&self) -> ImageSpec {
                self.0.image_spec()
            }
            fn capabilities(&self) -> Capabilities {
                self.0.capabilities()
            }
            fn identity(&self) -> Result<String, DeckError> {
                Err(DeckError::Unsupported {
                    operation: DeckOperation::Identity,
                })
            }
            fn set_brightness(&mut self, value: u8) -> Result<(), DeckError> {
                self.0.set_brightness(value)
            }
            fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
                self.0.set_key_image(key, jpeg)
            }
            fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
                self.0.clear_key(key)
            }
            fn clear_all(&mut self) -> Result<(), DeckError> {
                self.0.clear_all()
            }
            fn sleep(&mut self) -> Result<(), DeckError> {
                self.0.sleep()
            }
            fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError> {
                self.0.set_dial(dial, value)
            }
            fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
                self.0.poll_events(timeout)
            }
            fn reconnect(&mut self) -> Result<(), DeckError> {
                self.0.reconnect()
            }
        }

        let session = DeckSession::new(Box::new(Anonymous(SixKeyDeck::new())));

        assert_eq!(session.label(), "6 keys · 3 columns · 72x72 images");
        assert_eq!(session.key(), "shape: 6 keys · 3 columns · 72x72 images");
    }

    /// The `(width, height)` a JPEG declares in its frame header.
    ///
    /// Reading the size out of the bytes is the proof that the encoder was
    /// given the deck's numbers: a test that only checks the arguments would
    /// pass with an encoder that ignored them.
    fn jpeg_size(jpeg: &[u8]) -> (u16, u16) {
        assert!(jpeg.len() > 4, "not a JPEG: {jpeg:?}");
        let mut at = 2;
        loop {
            assert_eq!(jpeg[at], 0xFF, "expected a marker at byte {at}");
            let marker = jpeg[at + 1];
            at += 2;
            if marker == 0xD8 || marker == 0xD9 {
                continue;
            }
            let length = usize::from(u16::from_be_bytes([jpeg[at], jpeg[at + 1]]));
            let is_frame_header =
                (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if is_frame_header {
                let height = u16::from_be_bytes([jpeg[at + 3], jpeg[at + 4]]);
                let width = u16::from_be_bytes([jpeg[at + 5], jpeg[at + 6]]);
                return (width, height);
            }
            at += length;
            assert!(at + 4 < jpeg.len(), "no frame header in the JPEG");
        }
    }
}
