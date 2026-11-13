//! A deck with a different shape, and nothing else in common with the device
//! the seam was written for.
//!
//! Six keys in two rows of three, 72×72 images, two dials, no brightness and no
//! sleep. Every one of those is a difference the seam has to carry, so none of
//! the numbers is exposed as a constant a caller could lean on: the way to learn
//! this deck's shape is [`Deck::geometry`], [`Deck::image_spec`] and
//! [`Deck::capabilities`], which is the same way the window learns it.
//!
//! It is a test double, not an emulator: there is no transport, no clock and no
//! device. What it does keep is a record of the last image it was asked to paint
//! on each key, so a test can tell "the deck accepted it" from "the deck says it
//! accepted it".

use std::collections::VecDeque;
use std::time::Duration;

use deck_api::{
    Capabilities, Deck, DeckError, DeckOperation, Geometry, ImageFormat, ImageSpec, KeyEvent,
    KeyId, VALUE_MAX,
};

/// How many keys the deck has.
const KEY_COUNT: u16 = 6;

/// How many keys each row has. Two rows follow from the two numbers.
const COLUMNS: u16 = 3;

/// The width of one key's image, in pixels.
const IMAGE_WIDTH: u16 = 72;

/// The height of one key's image, in pixels.
const IMAGE_HEIGHT: u16 = 72;

/// The largest image the deck accepts, in bytes.
const IMAGE_MAX_BYTES: usize = 4_096;

/// How many dials the deck has, numbered from zero.
const DIAL_COUNT: u16 = 2;

/// What the deck calls itself.
const IDENTITY: &str = "deck-simulator: 6 keys, 2 dials";

/// A simulated deck whose shape is nothing like the first one.
///
/// ```
/// use deck_api::{Deck, KeyId};
/// use deck_simulator::SixKeyDeck;
///
/// let mut deck = SixKeyDeck::new();
/// assert_eq!(deck.geometry().key_count(), 6);
///
/// deck.set_key_image(KeyId::new(0), &[0xFF, 0xD8]).expect("key 0 exists");
/// assert_eq!(deck.painted(KeyId::new(0)), Some(&[0xFF_u8, 0xD8][..]));
/// ```
#[derive(Debug)]
pub struct SixKeyDeck {
    /// The last image each key was asked to show, in the deck's key order.
    paints: Vec<Option<Vec<u8>>>,
    /// The last position each dial was moved to.
    dials: Vec<Option<u8>>,
    /// The presses a test has queued, oldest first.
    events: VecDeque<KeyEvent>,
}

impl SixKeyDeck {
    /// Builds a deck with every key blank and no events waiting.
    ///
    /// ```
    /// use deck_api::Deck;
    /// use deck_simulator::SixKeyDeck;
    ///
    /// let deck = SixKeyDeck::new();
    /// assert_eq!(deck.capabilities().dial_count(), 2);
    /// ```
    pub fn new() -> Self {
        Self {
            paints: vec![None; usize::from(KEY_COUNT)],
            dials: vec![None; usize::from(DIAL_COUNT)],
            events: VecDeque::new(),
        }
    }

    /// The image the deck was last asked to show on `key`, or `None`.
    ///
    /// A key that has been painted and then cleared answers `None`, exactly as
    /// it would if it had never been painted: the record is the deck's state,
    /// not a log of calls.
    ///
    /// ```
    /// use deck_api::{Deck, KeyId};
    /// use deck_simulator::SixKeyDeck;
    ///
    /// let mut deck = SixKeyDeck::new();
    /// assert_eq!(deck.painted(KeyId::FIRST), None);
    ///
    /// deck.set_key_image(KeyId::FIRST, &[1, 2, 3]).expect("key 0 exists");
    /// assert_eq!(deck.painted(KeyId::FIRST), Some(&[1_u8, 2, 3][..]));
    /// ```
    pub fn painted(&self, key: KeyId) -> Option<&[u8]> {
        let index = usize::from(key.index());
        self.paints.get(index).and_then(|slot| slot.as_deref())
    }

    /// The keys that currently have an image, in the deck's key order.
    ///
    /// ```
    /// use deck_api::{Deck, KeyId};
    /// use deck_simulator::SixKeyDeck;
    ///
    /// let mut deck = SixKeyDeck::new();
    /// deck.set_key_image(KeyId::new(4), &[9]).expect("key 4 exists");
    /// assert_eq!(deck.painted_keys(), vec![KeyId::new(4)]);
    /// ```
    pub fn painted_keys(&self) -> Vec<KeyId> {
        (0..KEY_COUNT)
            .filter(|index| {
                self.paints
                    .get(usize::from(*index))
                    .is_some_and(Option::is_some)
            })
            .map(KeyId::new)
            .collect()
    }

    /// Queues one key transition for the next [`Deck::poll_events`].
    ///
    /// This is how a test presses a key: the deck has no hardware to press.
    ///
    /// ```
    /// use deck_api::{ButtonState, Deck, KeyEvent, KeyId};
    /// use deck_simulator::SixKeyDeck;
    ///
    /// let mut deck = SixKeyDeck::new();
    /// deck.queue_event(KeyEvent::new(KeyId::new(2), ButtonState::Pressed));
    ///
    /// let events = deck.poll_events(std::time::Duration::ZERO).expect("a poll succeeds");
    /// assert_eq!(events.len(), 1);
    /// ```
    pub fn queue_event(&mut self, event: KeyEvent) {
        self.events.push_back(event);
    }

    /// The position dial `dial` was last moved to, or `None`.
    ///
    /// ```
    /// use deck_api::Deck;
    /// use deck_simulator::SixKeyDeck;
    ///
    /// let mut deck = SixKeyDeck::new();
    /// deck.set_dial(1, 42).expect("dial 1 exists");
    /// assert_eq!(deck.dial_position(1), Some(42));
    /// ```
    pub fn dial_position(&self, dial: u8) -> Option<u8> {
        let index = usize::from(dial);
        self.dials.get(index).copied().flatten()
    }

    /// Refuses a key that does not exist, before anything is done with it.
    fn index_of(&self, key: KeyId) -> Result<usize, DeckError> {
        if key.index() >= KEY_COUNT {
            return Err(DeckError::KeyOutOfRange {
                key,
                key_count: KEY_COUNT,
            });
        }
        Ok(usize::from(key.index()))
    }
}

impl Default for SixKeyDeck {
    /// The same as [`SixKeyDeck::new`].
    ///
    /// ```
    /// use deck_api::Deck;
    /// use deck_simulator::SixKeyDeck;
    ///
    /// assert_eq!(SixKeyDeck::default().geometry().key_count(), 6);
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

impl Deck for SixKeyDeck {
    /// Six keys, three to a row.
    fn geometry(&self) -> Geometry {
        Geometry::new(KEY_COUNT, COLUMNS)
    }

    /// 72×72 JPEGs, at most four kilobytes each.
    fn image_spec(&self) -> ImageSpec {
        ImageSpec::new(
            IMAGE_WIDTH,
            IMAGE_HEIGHT,
            IMAGE_MAX_BYTES,
            ImageFormat::Jpeg,
        )
    }

    /// Single-key clear, clear-all and two dials; no brightness, no sleep.
    fn capabilities(&self) -> Capabilities {
        Capabilities::new()
            .with_clear_key(true)
            .with_clear_all(true)
            .with_dials(DIAL_COUNT)
    }

    /// The simulator's own name, so a caller has something to display.
    fn identity(&self) -> Result<String, DeckError> {
        Ok(IDENTITY.to_owned())
    }

    /// Always refused: this deck has no backlight to set.
    ///
    /// A brightness the deck cannot take is a typed refusal, never a success
    /// that changed nothing.
    fn set_brightness(&mut self, _value: u8) -> Result<(), DeckError> {
        Err(DeckError::Unsupported {
            operation: DeckOperation::Brightness,
        })
    }

    /// Paints one key, after refusing a key that does not exist and a payload
    /// the deck would not accept.
    ///
    /// The order is the cheap check first: the key is a comparison, the payload
    /// is a length. Nothing is stored until both have passed, so a refusal
    /// leaves the key showing whatever it showed before.
    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        let index = self.index_of(key)?;
        if jpeg.len() > IMAGE_MAX_BYTES {
            return Err(DeckError::ImageTooLarge {
                bytes: jpeg.len(),
                max_bytes: IMAGE_MAX_BYTES,
            });
        }
        let Some(slot) = self.paints.get_mut(index) else {
            // Unreachable: `index_of` already proved the index. Spelled out so
            // the refusal path stays the only way out, with no index panic.
            return Err(DeckError::KeyOutOfRange {
                key,
                key_count: KEY_COUNT,
            });
        };
        *slot = Some(jpeg.to_vec());
        Ok(())
    }

    /// Blanks one key, after refusing a key that does not exist.
    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
        let index = self.index_of(key)?;
        let Some(slot) = self.paints.get_mut(index) else {
            return Err(DeckError::KeyOutOfRange {
                key,
                key_count: KEY_COUNT,
            });
        };
        *slot = None;
        Ok(())
    }

    /// Blanks every key at once.
    fn clear_all(&mut self) -> Result<(), DeckError> {
        self.paints.fill(None);
        Ok(())
    }

    /// Always refused: this deck has no sleep command.
    fn sleep(&mut self) -> Result<(), DeckError> {
        Err(DeckError::Unsupported {
            operation: DeckOperation::Sleep,
        })
    }

    /// Moves one dial, after refusing a dial this deck does not have and a
    /// position past the shared range.
    fn set_dial(&mut self, dial: u8, value: u8) -> Result<(), DeckError> {
        if u16::from(dial) >= DIAL_COUNT {
            return Err(DeckError::DialOutOfRange {
                dial,
                dial_count: DIAL_COUNT,
            });
        }
        if value > VALUE_MAX {
            return Err(DeckError::value_out_of_range(DeckOperation::SetDial, value));
        }
        let index = usize::from(dial);
        let Some(slot) = self.dials.get_mut(index) else {
            return Err(DeckError::DialOutOfRange {
                dial,
                dial_count: DIAL_COUNT,
            });
        };
        *slot = Some(value);
        Ok(())
    }

    /// Hands over the queue and returns immediately.
    ///
    /// The timeout is ignored on purpose: this deck has no device to wait for,
    /// and a test double that sleeps is a test suite that sleeps. Nothing
    /// waiting is an empty list, which is the normal answer.
    fn poll_events(&mut self, _timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        Ok(self.events.drain(..).collect())
    }

    /// Succeeds: there is no cable to pull on a simulator.
    fn reconnect(&mut self) -> Result<(), DeckError> {
        Ok(())
    }
}
