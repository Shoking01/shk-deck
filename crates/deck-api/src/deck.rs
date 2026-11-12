//! The deck trait: what a deck can do.

use std::time::Duration;

use crate::error::DeckError;
use crate::types::{Capabilities, Geometry, ImageSpec, KeyEvent, KeyId};

/// A deck: the semantics of one streamdeck model.
///
/// Everything a caller needs to adapt to the deck in front of it is here, as
/// data or as a typed error. Nothing in this trait is about bytes: that is
/// [`Transport`](crate::Transport)'s job, and a deck built on one keeps its
/// protocol to itself.
///
/// An implementation that cannot do something returns
/// [`DeckError::Unsupported`]. It does not accept the call and quietly do
/// nothing: a caller that cannot tell the difference between "done" and "not
/// possible" has no way to tell the user anything useful.
pub trait Deck {
    /// The shape of the deck's keys.
    fn geometry(&self) -> Geometry;

    /// The image the deck accepts for one key.
    fn image_spec(&self) -> ImageSpec;

    /// What the deck can do.
    fn capabilities(&self) -> Capabilities;

    /// The deck's identity string, as the device reports it.
    fn identity(&self) -> Result<String, DeckError>;

    /// Sets the display brightness, from 0 to 100.
    fn set_brightness(&mut self, value: u8) -> Result<(), DeckError>;

    /// Paints one key with a JPEG image.
    ///
    /// The image must match the deck's [`ImageSpec`]; a deck that takes
    /// another size or a bigger payload refuses it with a typed error.
    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError>;

    /// Clears one key back to blank.
    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError>;

    /// Clears every key back to blank.
    fn clear_all(&mut self) -> Result<(), DeckError>;

    /// Reads the key events waiting on the deck.
    ///
    /// A timeout with nothing waiting is an empty list, not an error.
    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError>;

    /// Reopens the device after a disconnect.
    fn reconnect(&mut self) -> Result<(), DeckError>;
}

/// A deck held behind one box.
///
/// This is the handle for code that knows "a deck" but not which one: the
/// window, the engine and the action runner can all hold a `DeckHandle`
/// without naming a model. The handle is a [`Deck`] itself, so it can be
/// used wherever a deck is expected.
pub struct DeckHandle {
    inner: Box<dyn Deck>,
}

impl DeckHandle {
    /// Boxes a deck.
    ///
    /// ```
    /// use deck_api::{DeckHandle, Geometry};
    ///
    /// fn handle_of(deck: Box<dyn deck_api::Deck>) -> DeckHandle {
    ///     DeckHandle::new(deck)
    /// }
    /// ```
    pub fn new(inner: Box<dyn Deck>) -> Self {
        Self { inner }
    }

    /// The geometry of the deck inside the handle.
    ///
    /// ```
    /// use deck_api::DeckHandle;
    ///
    /// fn geometry_of(handle: &DeckHandle) -> deck_api::Geometry {
    ///     handle.geometry()
    /// }
    /// ```
    pub fn geometry(&self) -> Geometry {
        self.inner.geometry()
    }
}

impl Deck for DeckHandle {
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
        self.inner.set_brightness(value)
    }

    fn set_key_image(&mut self, key: KeyId, jpeg: &[u8]) -> Result<(), DeckError> {
        self.inner.set_key_image(key, jpeg)
    }

    fn clear_key(&mut self, key: KeyId) -> Result<(), DeckError> {
        self.inner.clear_key(key)
    }

    fn clear_all(&mut self) -> Result<(), DeckError> {
        self.inner.clear_all()
    }

    fn poll_events(&mut self, timeout: Duration) -> Result<Vec<KeyEvent>, DeckError> {
        self.inner.poll_events(timeout)
    }

    fn reconnect(&mut self) -> Result<(), DeckError> {
        self.inner.reconnect()
    }
}
